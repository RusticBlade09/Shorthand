# Shorthand

Small Windows app (Rust + Tauri 2). Save phrases you type often under a short name.
Anywhere in Windows, type `//` → a chime plays and a small list pops up → type part of
the name, press **Enter** → the `//` is removed and the phrase is pasted where you were typing.
**Esc** closes the list without inserting anything.

## Download
Get the installer from the [Releases page](https://github.com/RusticBlade09/Shorthand/releases)
(`Shorthand_x.y.z_x64-setup.exe`). Windows 10/11, 64-bit. The installer is not code-signed, so
Windows SmartScreen may warn the first time: click **More info → Run anyway**.

Example: save a command called `wifi` with this text:

```
Our Wi-Fi details are:
Network: CoffeeHouse-Guest
Password: latte-1234
```

Then, in any chat or email, type `//`, then `wifi`, and press Enter: all three lines appear.

## How it works
- `ui/index.html` — main window: add / edit / delete commands.
- `ui/popup.html` — the `//` picker (filter, ↑↓, Enter, Esc).
- `src-tauri/src/lib.rs` — everything else: global keyboard hook (`rdev`), the `//` detector
  (`is_trigger`: the two slashes must come within 2 s of each other — `MAX_GAP` — and `https://` is ignored), beep, storage, paste (clipboard + Ctrl+V), tray icon, autostart.
- Closing the window hides it to the tray (by the clock). Tray menu: **Open** / **Quit**.
- Look: Windows 11 Fluent (same tokens as the author's Ramble app) — Mica window,
  the user's Windows accent colour + light/dark (`system_colors` in lib.rs, applied by `ui/theme.js`).
- The installed app adds itself to Windows startup and starts hidden (`--hidden`).

- Sound: our own chime (`src-tauri/sounds/chime.wav`, two soft rising notes) so `//` never sounds
  like a Windows notification. Regenerate with `node tools/make-chime.mjs`.
- Icon: art in `assets/icon-source.jpg` (glowing S on black). `tools/make-icon.py` crops it square and
  removes the black (needs Pillow) → `src-tauri/icons/source.png`; then
  `cd src-tauri && cargo tauri icon icons/source.png` (delete the `android`/`ios` folders it makes).
  `cargo tauri dev` does not notice icon changes — touch `build.rs` to force a rebuild.
- Esc in the picker returns focus to the window you were typing in; clicking away does not.
- Optional **shortcut key** (sidebar → Shortcut key: click, hold Ctrl and/or Alt, press a key) opens
  the list too, without typing `//`. Backspace in that box removes it.
- The clipboard is put back after pasting (text or images; copied files are not restored).
- Only one copy runs: starting it again just shows the open window.

## Data
`%APPDATA%\com.shorthand.app\commands.json` — `[{ "name": "...", "text": "..." }]`.
`%APPDATA%\com.shorthand.app\settings.json` — `{ "chime": true, "shortcut": "Ctrl+Shift+Space" }` (sidebar settings).
Both saved via a temp file + rename, so a crash can't wipe them.

## Build / run
```bash
cd src-tauri
cargo test          # detector test
cargo tauri dev     # run while developing
cargo tauri build   # installer → src-tauri/target/release/bundle/{nsis,msi}/
```
For a release, use `powershell -File tools\build-release.ps1` instead of `cargo tauri build`:
it keeps your Windows user folder path (and so your name) out of `shorthand.exe`.

## Known limits
- If you had files (not text or an image) copied, pasting a command replaces them on the clipboard.
- Typing `//` in code or a path also opens the list (Esc closes it).
- To stop it starting with Windows: Task Manager → Startup apps → Shorthand → Disable.
