$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$clangBin = 'C:/devkitPro/msys2/clang64/bin'
$program = Join-Path $root 'target/mesa-tools/mesa-abi-tests.exe'
& "$clangBin/clang++.exe" -std=c++17 (Join-Path $PSScriptRoot 'mesa_abi_tests.cpp') -o $program
if ($LASTEXITCODE -ne 0) { throw 'Cannot compile Mesa ABI host test' }
$previous = $env:PATH
try {
    $env:PATH = "$clangBin;$env:PATH"
    & $program
    if ($LASTEXITCODE -ne 0) { throw 'Mesa ABI host test failed' }
} finally { $env:PATH = $previous }
