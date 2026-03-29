# Download ONNX Runtime DirectML DLLs from NuGet.
# These are required for GPU-accelerated inference on AMD/Intel/NVIDIA GPUs.
$ErrorActionPreference = 'Stop'

$OrtVersion = "1.24.4"
$NugetUrl = "https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime.DirectML/$OrtVersion"
$OutDir = (Resolve-Path "$PSScriptRoot\..\core\rust-engine\ort-dml-libs").Path

if (Test-Path "$OutDir\onnxruntime.dll") {
    Write-Host "[SETUP] DirectML DLLs already present in $OutDir, skipping."
    exit 0
}

Write-Host "[SETUP] Downloading ORT DirectML NuGet v$OrtVersion..."
$TempZip = "$env:TEMP\ort-dml-$OrtVersion.zip"
$TempDir = "$env:TEMP\ort-dml-$OrtVersion"

Invoke-WebRequest -Uri $NugetUrl -OutFile $TempZip
Remove-Item -Recurse -Force $TempDir -ErrorAction SilentlyContinue
Expand-Archive $TempZip -DestinationPath $TempDir -Force

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
Copy-Item "$TempDir\runtimes\win-x64\native\*" $OutDir

Write-Host "[SETUP] DirectML DLLs extracted to $OutDir:"
Get-ChildItem $OutDir | ForEach-Object {
    Write-Host "  $($_.Name) ($([math]::Round($_.Length/1MB, 1)) MB)"
}
