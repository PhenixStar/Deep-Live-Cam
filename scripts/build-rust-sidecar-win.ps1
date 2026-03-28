# Build the Rust server binary as a Tauri sidecar on Windows.
# Produces: app/src-tauri/binaries/deep-live-cam-server-x86_64-pc-windows-msvc.exe
$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$BinariesDir = "$RepoRoot\app\src-tauri\binaries"

Write-Host "[BUILD] Building Rust server for Windows x86_64..."
Push-Location "$RepoRoot\core\rust-engine"
cargo build --release -p dlc-server
Pop-Location

Write-Host "[BUILD] Copying binary to Tauri binaries..."
New-Item -ItemType Directory -Force -Path $BinariesDir | Out-Null
Copy-Item "$RepoRoot\core\rust-engine\target\release\dlc-server.exe" "$BinariesDir\deep-live-cam-server-x86_64-pc-windows-msvc.exe"

Write-Host "[BUILD] Done."
