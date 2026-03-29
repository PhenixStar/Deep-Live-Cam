# Build the Rust server binary as a Tauri sidecar on Windows.
# Produces: app/src-tauri/binaries/deep-forge-server-x86_64-pc-windows-msvc.exe
$ErrorActionPreference = 'Stop'

$RepoRoot = (Resolve-Path "$PSScriptRoot\..").Path
$BinariesDir = "$RepoRoot\app\src-tauri\binaries"
$OrtLibsDir = "$RepoRoot\core\rust-engine\ort-dml-libs"

# Ensure DirectML DLLs are available
if (-not (Test-Path "$OrtLibsDir\onnxruntime.dll")) {
    Write-Host "[BUILD] DirectML DLLs not found. Running setup..."
    & pwsh "$PSScriptRoot\setup-directml-dlls.ps1"
}

Write-Host "[BUILD] Building Rust server for Windows x86_64..."
Push-Location "$RepoRoot\core\rust-engine"
$env:ORT_LIB_PATH = $OrtLibsDir
cargo build --release -p deep-forge-server
Pop-Location

Write-Host "[BUILD] Copying binary + DLLs to Tauri binaries..."
New-Item -ItemType Directory -Force -Path $BinariesDir | Out-Null
Copy-Item "$RepoRoot\core\rust-engine\target\release\deep-forge-server.exe" "$BinariesDir\deep-forge-server-x86_64-pc-windows-msvc.exe"

# Copy DirectML DLLs next to the binary (required at runtime)
Copy-Item "$OrtLibsDir\*.dll" $BinariesDir -Force

Write-Host "[BUILD] Done."
