# Rebuilds Recall and swaps the new recall.exe into the installed copy, then
# starts it. For trying changes without running the installer each time: data,
# settings and sign-ins stay as they are. Run from anywhere:
#   powershell -ExecutionPolicy Bypass -File scripts\update-installed.ps1
$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
$installed = Join-Path $env:LOCALAPPDATA "Recall\recall.exe"
if (-not (Test-Path $installed)) { throw "Recall is not installed at $installed. Run the installer once first." }

Write-Host "Building (a few minutes the first time)..."
Push-Location $repo
try {
    & npx tauri build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
} finally { Pop-Location }

Write-Host "Closing Recall..."
# ask the window to close first; a hidden sign-in window keeps Recall running
# after that, so it is then stopped
Get-Process recall -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $installed } | ForEach-Object {
    $null = $_.CloseMainWindow()
    if (-not $_.WaitForExit(5000)) { Stop-Process -Id $_.Id -Force; $null = $_.WaitForExit(5000) }
}
# sign-in windows run in WebView2 helper processes that can outlive Recall and
# keep its profile busy, which makes the next start fail
Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" |
    Where-Object { $_.CommandLine -match 'app\.recall\.desktop' } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Start-Sleep -Seconds 2

# Windows can hold the old exe for a few seconds after the process ends
$built = Join-Path $repo "target\release\recall.exe"
for ($i = 0; ; $i++) {
    try { Copy-Item $built $installed -Force; break }
    catch { if ($i -ge 20) { throw }; Start-Sleep -Seconds 1 }
}
Start-Process $installed
Write-Host "Updated and started. Log: $(Join-Path $env:LOCALAPPDATA 'Recall\recall.log')"
