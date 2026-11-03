# Builds the installer without your Windows user folder (and so your name) baked into shorthand.exe.
# Rust embeds source paths in panic messages; --remap-path-prefix swaps the user folder for "~",
# and /PDBALTPATH keeps only the debug file's name instead of its full path.
# CARGO_ENCODED_RUSTFLAGS (flags split by 0x1F) because the user folder may contain a space.
# Run from anywhere: powershell -File tools\build-release.ps1
$env:CARGO_ENCODED_RUSTFLAGS = @("--remap-path-prefix=$env:USERPROFILE=~", '-C', 'link-arg=/PDBALTPATH:%_PDB%') -join [char]0x1F
Push-Location (Join-Path $PSScriptRoot '..\src-tauri')
try { cargo tauri build } finally { Pop-Location }
