# Builds and runs the host tests of app/engine/update_core.h (MinGW g++). Optional: pass a Blogger feed file to parse.
param([string]$Feed)
$here = $PSScriptRoot
Push-Location $here
try {
    g++ -std=c++17 -O1 -Wall -Wextra -o test.exe test.cpp
    if ($LASTEXITCODE -ne 0) { throw 'update_core test build failed' }
    if ($Feed) { ./test.exe $Feed } else { ./test.exe }
    if ($LASTEXITCODE -ne 0) { throw 'update_core tests failed' }
} finally { Pop-Location }
