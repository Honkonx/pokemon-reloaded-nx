# Builds and runs the menu preview on the PC (MinGW g++ from WinGet).
$here = $PSScriptRoot
New-Item -ItemType Directory -Force -Path (Join-Path $here 'out') | Out-Null
Push-Location $here
try {
    g++ -std=c++17 -O1 -DPOKENX_PREVIEW=1 -DPOKENX_KEYS_REPORT_PATH='"out/game-keys.txt"' -DPOKENX_LIVE_KEYS_PATH='"out/game-keys-live.txt"' -DPOKENX_COMPAT_REPORT_PATH='"out/game-compat.txt"' -I stub -I ../../app/engine -o preview.exe preview.cpp ../../app/engine/gfx.cpp
    if ($LASTEXITCODE -ne 0) { throw 'preview build failed' }
    ./preview.exe
} finally { Pop-Location }
