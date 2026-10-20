# Builds and runs the checks of the in-memory NCA builder (app/engine/home_nca.h) on the PC (MinGW g++ or any g++).
$here = $PSScriptRoot
Push-Location $here
try {
    g++ -std=c++17 -O1 -Wall -Wextra -I ../../app/engine -o home_nca_test.exe test.cpp
    if ($LASTEXITCODE -ne 0) { throw 'home_nca_test build failed' }
    ./home_nca_test.exe
    if ($LASTEXITCODE -ne 0) { throw 'home_nca_test failed' }
} finally { Pop-Location }
