// Match Windows like Ramble does: light/dark + the user's accent colour (system_colors in lib.rs).
// matchMedia is the instant guess that avoids a flash; the Windows read corrects it.
const darkQuery = matchMedia('(prefers-color-scheme: dark)');
document.body.classList.toggle('light', !darkQuery.matches);
darkQuery.addEventListener('change', e => document.body.classList.toggle('light', !e.matches));
window.__TAURI__.core.invoke('system_colors').then(c => {
  const root = document.documentElement.style;
  root.setProperty('--sys-accent-light2', c.light2);
  root.setProperty('--sys-accent-dark1', c.dark1);
  document.body.classList.toggle('light', !c.dark);
}).catch(() => {}); // the stylesheet already holds Windows' default blue
