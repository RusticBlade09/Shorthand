use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering::SeqCst};
use std::thread;
use std::time::{Duration, Instant};

use rdev::{EventType, Key};
use serde_json::{json, Value};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, SetForegroundWindow};

// Our own sound (made by tools/make-chime.mjs) so "//" never sounds like a Windows notification.
static CHIME: &[u8] = include_bytes!("../sounds/chime.wav");

// The window the user was typing in when they pressed "//".
static TARGET: AtomicIsize = AtomicIsize::new(0);
// Kept in an atomic so the keyboard hook never waits on the UI thread.
static POPUP_OPEN: AtomicBool = AtomicBool::new(false);
// Chime on/off, from settings.json (loaded at startup, changed by set_chime).
static CHIME_ON: AtomicBool = AtomicBool::new(true);
// Whether the open picker came from typing "//" (see open_popup).
static TYPED: AtomicBool = AtomicBool::new(true);

// The two slashes must come quickly; a "/" typed long after another one is just text.
const MAX_GAP: Duration = Duration::from_secs(2);

/// "//" typed within MAX_GAP fires, but not the "//" in "https://".
fn is_trigger(prev2: Option<char>, prev: Option<char>, now: char, gap: Duration) -> bool {
  now == '/' && prev == Some('/') && prev2 != Some(':') && gap <= MAX_GAP
}

fn data_file(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
  let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
  fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
  Ok(dir.join(name))
}

fn read_json(app: &AppHandle, name: &str, missing: Value) -> Result<Value, String> {
  match fs::read_to_string(data_file(app, name)?) {
    Ok(s) => serde_json::from_str(&s).map_err(|e| e.to_string()),
    Err(e) if e.kind() == ErrorKind::NotFound => Ok(missing),
    Err(e) => Err(e.to_string()),
  }
}

fn write_json(app: &AppHandle, name: &str, value: &Value) -> Result<(), String> {
  let path = data_file(app, name)?;
  // Write to a temp file first so a crash mid-write can't wipe what was saved.
  let tmp = path.with_extension("tmp");
  fs::write(&tmp, serde_json::to_string_pretty(value).map_err(|e| e.to_string())?)
    .map_err(|e| e.to_string())?;
  fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_commands(app: AppHandle) -> Result<Value, String> {
  read_json(&app, "commands.json", json!([]))
}

#[tauri::command]
fn save_commands(app: AppHandle, list: Value) -> Result<(), String> {
  write_json(&app, "commands.json", &list)
}

/// settings.json, e.g. { "chime": true, "shortcut": "Ctrl+Shift+Space" }. Missing or unreadable = defaults.
#[tauri::command]
fn get_settings(app: AppHandle) -> Value {
  read_json(&app, "settings.json", json!({})).unwrap_or_else(|_| json!({}))
}

fn save_setting(app: &AppHandle, key: &str, value: Value) -> Result<(), String> {
  let mut settings = get_settings(app.clone());
  if !settings.is_object() {
    settings = json!({});
  }
  settings[key] = value;
  write_json(app, "settings.json", &settings)
}

#[tauri::command]
fn set_chime(app: AppHandle, on: bool) -> Result<(), String> {
  save_setting(&app, "chime", json!(on))?;
  CHIME_ON.store(on, SeqCst);
  Ok(())
}

/// Register `shortcut` (e.g. "Ctrl+Shift+Space", "" = none) as an extra way to open the picker.
fn register_shortcut(app: &AppHandle, shortcut: &str) -> Result<(), String> {
  let keys = app.global_shortcut();
  keys.unregister_all().map_err(|e| e.to_string())?;
  if shortcut.is_empty() {
    return Ok(());
  }
  keys.register(shortcut).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_shortcut(app: AppHandle, shortcut: String) -> Result<(), String> {
  if let Err(e) = register_shortcut(&app, &shortcut) {
    // Usually another app owns that combination; keep the old shortcut working.
    let old = get_settings(app.clone())["shortcut"].as_str().unwrap_or("").to_string();
    let _ = register_shortcut(&app, &old);
    return Err(e);
  }
  save_setting(&app, "shortcut", json!(shortcut))
}

/// Windows' accent shades + light/dark, so the UI matches the OS (same source as Ramble's theme.rs).
/// Fluent paints AccentLight2 in dark mode and AccentDark1 in light mode, never the raw accent.
#[tauri::command]
fn system_colors() -> Result<Value, String> {
  use windows::UI::ViewManagement::{UIColorType, UISettings};
  let s = UISettings::new().map_err(|e| e.to_string())?;
  let get = |t| s.GetColorValue(t).map_err(|e| e.to_string());
  let hex = |c: windows::UI::Color| format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B);
  Ok(json!({
    "light2": hex(get(UIColorType::AccentLight2)?),
    "dark1": hex(get(UIColorType::AccentDark1)?),
    "dark": get(UIColorType::Background)?.R < 128, // Background is black in dark mode
  }))
}

/// `restore` hands focus back to the window the user was typing in (Esc / insert),
/// but not when they clicked away to somewhere else.
#[tauri::command]
fn hide_popup(app: AppHandle, restore: bool) {
  if let Some(w) = app.get_webview_window("popup") {
    let _ = w.hide();
  }
  POPUP_OPEN.store(false, SeqCst);
  if restore {
    unsafe { SetForegroundWindow(TARGET.load(SeqCst) as _) };
  }
}

fn send(ev: EventType) {
  let _ = rdev::simulate(&ev);
  thread::sleep(Duration::from_millis(20));
}

fn tap(k: Key) {
  send(EventType::KeyPress(k));
  send(EventType::KeyRelease(k));
}

#[tauri::command]
async fn insert(app: AppHandle, text: String) -> Result<(), String> {
  hide_popup(app, true);
  thread::sleep(Duration::from_millis(150));
  // Paste instead of typing: typed line breaks would press Enter (= "send" in chat apps).
  let mut clip = arboard::Clipboard::new().map_err(|e| e.to_string())?;
  // shortcut: only text or an image is put back afterwards; copied files are lost, rare enough for now.
  let old_text = clip.get_text().ok();
  let old_image = if old_text.is_none() { clip.get_image().ok() } else { None };
  clip.set_text(text).map_err(|e| e.to_string())?;
  if TYPED.load(SeqCst) {
    tap(Key::Backspace); // remove the "//" the user typed
    tap(Key::Backspace);
  }
  send(EventType::KeyPress(Key::ControlLeft));
  tap(Key::KeyV);
  send(EventType::KeyRelease(Key::ControlLeft));
  thread::sleep(Duration::from_millis(400)); // the app reads the clipboard after Ctrl+V; give it time
  let _ = match (old_text, old_image) {
    (Some(t), _) => clip.set_text(t),
    (None, Some(img)) => clip.set_image(img),
    (None, None) => clip.clear(),
  };
  Ok(())
}

/// `typed`: opened by typing "//" (those two characters must be removed before pasting),
/// rather than by the shortcut key.
fn open_popup(app: &AppHandle, typed: bool) {
  TYPED.store(typed, SeqCst);
  TARGET.store(unsafe { GetForegroundWindow() } as isize, SeqCst);
  POPUP_OPEN.store(true, SeqCst);
  if CHIME_ON.load(SeqCst) {
    unsafe { PlaySoundW(CHIME.as_ptr() as _, std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
  }
  let app = app.clone();
  thread::spawn(move || {
    thread::sleep(Duration::from_millis(50)); // let the second "/" reach the target app first
    if let Some(w) = app.get_webview_window("popup") {
      center_on_target_monitor(&w);
      let _ = w.show();
      let _ = w.set_focus();
      let _ = w.emit("popup-open", ());
    }
  });
}

/// Top-left corner that centres a `size` window inside the `area` rectangle (physical pixels).
fn centered(area_pos: (i32, i32), area_size: (u32, u32), size: (f64, f64)) -> (i32, i32) {
  let x = area_pos.0 as f64 + (area_size.0 as f64 - size.0) / 2.0;
  let y = area_pos.1 as f64 + (area_size.1 as f64 - size.1) / 2.0;
  (x.round() as i32, y.round() as i32)
}

/// Put the picker in the middle of the screen that holds the app the user was typing in,
/// not wherever the picker was last shown.
fn center_on_target_monitor(w: &WebviewWindow) {
  let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
  let target = TARGET.load(SeqCst);
  let monitor = if unsafe { GetWindowRect(target as _, &mut r) } != 0 {
    let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    w.monitor_from_point(cx as f64, cy as f64).ok().flatten()
  } else {
    None
  };
  let Some(m) = monitor.or_else(|| w.primary_monitor().ok().flatten()) else {
    let _ = w.center();
    return;
  };
  // The picker takes the target screen's scaling once it lands there, so size it for that screen.
  let scale = m.scale_factor() / w.scale_factor().unwrap_or(1.0);
  let size = w.outer_size().unwrap_or_default();
  let area = m.work_area();
  let (x, y) = centered(
    (area.position.x, area.position.y),
    (area.size.width, area.size.height),
    (size.width as f64 * scale, size.height as f64 * scale),
  );
  let _ = w.set_position(PhysicalPosition::new(x, y));
}

fn start_hook(app: AppHandle) {
  thread::spawn(move || {
    let (mut prev2, mut prev) = (None, None);
    let mut prev_at = Instant::now();
    let _ = rdev::listen(move |ev| {
      if !matches!(ev.event_type, EventType::KeyPress(_)) || POPUP_OPEN.load(SeqCst) {
        return;
      }
      let Some(name) = ev.name else { return };
      let mut chars = name.chars();
      let (Some(c), None) = (chars.next(), chars.next()) else { return };
      let now = Instant::now();
      if is_trigger(prev2, prev, c, now - prev_at) {
        (prev2, prev) = (None, None);
        open_popup(&app, true);
      } else {
        (prev2, prev) = (prev, Some(c));
      }
      prev_at = now;
    });
  });
}

fn show_main(app: &AppHandle) {
  if let Some(w) = app.get_webview_window("main") {
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
  }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    // Must come first: a second copy hands over to the running one (shows its window) and quits,
    // so two keyboard hooks never open two pickers.
    .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
      if !args.iter().any(|a| a == "--hidden") {
        show_main(app);
      }
    }))
    .plugin(tauri_plugin_autostart::init(
      tauri_plugin_autostart::MacosLauncher::LaunchAgent,
      Some(vec!["--hidden"]),
    ))
    .plugin(
      tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
          if event.state == ShortcutState::Pressed && !POPUP_OPEN.load(SeqCst) {
            open_popup(app, false);
          }
        })
        .build(),
    )
    .invoke_handler(tauri::generate_handler![
      load_commands,
      save_commands,
      get_settings,
      set_chime,
      set_shortcut,
      system_colors,
      hide_popup,
      insert
    ])
    .on_window_event(|window, event| match event {
      // Closing only hides: the app keeps listening for "//" from the tray.
      WindowEvent::CloseRequested { api, .. } => {
        api.prevent_close();
        let _ = window.hide();
      }
      WindowEvent::Focused(false) if window.label() == "popup" => hide_popup(window.app_handle().clone(), false),
      _ => {}
    })
    .setup(|app| {
      let menu = Menu::with_items(
        app,
        &[
          &MenuItem::with_id(app, "open", "Open", true, None::<&str>)?,
          &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
      )?;
      TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Shorthand — type // to insert a command")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
          "open" => show_main(app),
          "quit" => app.exit(0),
          _ => {}
        })
        .on_tray_icon_event(|tray, e| {
          if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            show_main(tray.app_handle());
          }
        })
        .build(app)?;

      // Only the installed app registers itself, so dev builds never land in Windows startup.
      #[cfg(not(debug_assertions))]
      {
        use tauri_plugin_autostart::ManagerExt;
        let auto = app.autolaunch();
        if !auto.is_enabled()? {
          auto.enable()?;
        }
      }

      // A missing or unreadable settings file just means the defaults: chime on, no shortcut.
      let settings = get_settings(app.handle().clone());
      CHIME_ON.store(settings["chime"].as_bool().unwrap_or(true), SeqCst);
      // A shortcut another app has taken since is skipped; "//" still works.
      let _ = register_shortcut(app.handle(), settings["shortcut"].as_str().unwrap_or(""));

      if !std::env::args().any(|a| a == "--hidden") {
        show_main(app.handle());
      }
      start_hook(app.handle().clone());
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
  use super::{centered, is_trigger};
  use std::time::Duration;
  use tauri_plugin_global_shortcut::Shortcut;

  #[test]
  fn shortcut_format_from_the_ui_parses() {
    // index.html builds these strings from the keys pressed; the plugin must accept them.
    for s in ["Ctrl+Shift+Space", "Ctrl+K", "Alt+1", "Ctrl+Alt+F5", "Ctrl+Backquote"] {
      assert!(s.parse::<Shortcut>().is_ok(), "{s}");
    }
  }

  #[test]
  fn centre_on_screen() {
    // A 440x320 picker on a 1920x1040 work area that sits above the primary screen.
    assert_eq!(centered((0, -1080), (1920, 1040), (440.0, 320.0)), (740, -720));
    // A screen to the left of the primary one, at 150% scaling (660x480 physical).
    assert_eq!(centered((-2560, 0), (2560, 1400), (660.0, 480.0)), (-1610, 460));
  }

  #[test]
  fn trigger() {
    let quick = Duration::from_millis(150);
    assert!(is_trigger(None, Some('/'), '/', quick));
    assert!(is_trigger(Some(' '), Some('/'), '/', quick));
    assert!(is_trigger(None, Some('/'), '/', Duration::from_secs(2))); // right on the limit
    assert!(!is_trigger(None, Some('/'), '/', Duration::from_millis(2001))); // paused too long
    assert!(!is_trigger(Some(':'), Some('/'), '/', quick)); // https://
    assert!(!is_trigger(Some('/'), Some('x'), '/', quick)); // /x/
    assert!(!is_trigger(None, None, '/', quick));
  }
}
