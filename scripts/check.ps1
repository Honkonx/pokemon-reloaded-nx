# Automatic checks that need neither a Switch nor the game. Each stage is reported separately:
#   compila_host   : unit tests of the runtime crates and the menu preview (menu drawn on the PC)
#   compila_switch : cross-compilation of the engine for Horizon (aarch64-nintendo-switch-freestanding)
# None of this proves that the NRO starts or plays on a console.
# Usage: pwsh scripts/check.ps1 [-SkipCross] [-SkipHost]
param([switch]$SkipCross, [switch]$SkipHost)
$ErrorActionPreference = 'Continue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$results = New-Object System.Collections.Generic.List[object]
$toolchain = '+nightly-2026-09-29'

function Invoke-Stage($stage, $name, $dir, [scriptblock]$command) {
    Push-Location $dir
    try {
        $output = & $command 2>&1 | Out-String
        $ok = ($LASTEXITCODE -eq 0)
    } catch { $output = $_.Exception.Message; $ok = $false } finally { Pop-Location }
    $summary = ($output -split "`n" | Where-Object { $_ -match '^test result:|^error(\[|:)|^error: could not compile' } | Select-Object -First 3) -join ' | '
    $results.Add([pscustomobject]@{ Stage = $stage; Target = $name; Result = $(if ($ok) { 'OK' } else { 'FAIL' }); Detail = $summary })
    if (-not $ok) { Write-Host "---- output of $name ----"; Write-Host $output }
}

if (-not $SkipHost) {
    $crates = 'gm8-input', 'gm8-audio-output', 'gm8-real', 'gm8exe', 'gml-parser', 'pokenx-io', 'pokenx-std', 'pokenx-core', 'pokenx-engine',
              'gm8-format-probe', 'gm8-binary', 'gm8-zlib', 'gm8-assets', 'gm8-background', 'gm8-font', 'gm8-object', 'gm8-path',
              'gm8-resource-loader', 'gm8-room', 'gm8-script', 'gm8-sound', 'gm8-sprite', 'gm8-timeline', 'switch-pe-probe'
    foreach ($crate in $crates) {
        $manifest = Join-Path $root "src/gm8-runtime/$crate/Cargo.toml"
        if (Test-Path -LiteralPath $manifest) {
            Invoke-Stage 'compila_host' "src/gm8-runtime/$crate" $root { cargo $toolchain test --quiet --manifest-path $manifest }
        }
    }
    # The graphical menu (gfx.cpp + menu.cpp) compiled and drawn on the PC with libnx replaced by tools/menu_preview/stub.
    if (Get-Command g++ -ErrorAction SilentlyContinue) {
        Invoke-Stage 'compila_host' 'graphical menu (PC preview)' (Join-Path $root 'tools/menu_preview') {
            New-Item -ItemType Directory -Force -Path out | Out-Null
            g++ -std=c++17 -O1 -DPOKENX_PREVIEW=1 -DPOKENX_KEYS_REPORT_PATH='"out/game-keys.txt"' -DPOKENX_LIVE_KEYS_PATH='"out/game-keys-live.txt"' -DPOKENX_COMPAT_REPORT_PATH='"out/game-compat.txt"' -I stub -I ../../app/engine -o preview.exe preview.cpp ../../app/engine/gfx.cpp
            if ($LASTEXITCODE -ne 0) { throw 'the preview does not compile' }
            ./preview.exe
            if ($LASTEXITCODE -ne 0) { throw 'the preview failed' }
            if (-not (Test-Path out/01-inicio.png)) { throw 'no capture was written' }
            $global:LASTEXITCODE = 0
        }
    }
}

if (-not $SkipCross) {
    $engineManifest = Join-Path $root 'src/gm8-runtime/pokenx-engine/Cargo.toml'
    Invoke-Stage 'compila_switch' 'pokenx-engine staticlib (feature switch)' $root {
        cargo $toolchain check --release --manifest-path $engineManifest --no-default-features --features switch `
            --target aarch64-nintendo-switch-freestanding -Z build-std=core,alloc,compiler_builtins
    }
    foreach ($crate in 'pokenx-io', 'pokenx-std', 'gm8exe', 'pokenx-core') {
        $manifest = Join-Path $root "src/gm8-runtime/$crate/Cargo.toml"
        Invoke-Stage 'compila_switch' "$crate (no_std)" $root {
            cargo $toolchain check --release --manifest-path $manifest --no-default-features `
                --target aarch64-nintendo-switch-freestanding -Z build-std=core,alloc,compiler_builtins `
                -Z build-std-features=compiler-builtins-mem
        }
    }
}

# Every relocation of the linked ELF must fall in a writable segment, or the NRO dies at start-up (see docs/BUILDING.md).
$engineElf = Join-Path $root 'app/engine/target/pokenx-engine.elf'
if (Test-Path -LiteralPath $engineElf -PathType Leaf) {
    Invoke-Stage 'compila_switch' 'ELF relocations of the engine' $root {
        python (Join-Path $root 'scripts/verify_relocations.py') $engineElf
        if ($LASTEXITCODE -ne 0) { throw 'relocations outside writable segments' }
    }
}

# The updater's pure logic (versions, the blog feed, links and Mega replies) without network or libnx.
if (Get-Command g++ -ErrorAction SilentlyContinue) {
    Invoke-Stage 'compila_host' 'updater: pure logic (update_core, no network)' (Join-Path $root 'tools/update_core_test') {
        g++ -std=c++17 -O1 -Wall -Wextra -o test.exe test.cpp
        if ($LASTEXITCODE -ne 0) { throw 'update_core does not compile' }
        ./test.exe
        if ($LASTEXITCODE -ne 0) { throw 'update_core: tests failed' }
    }
}

# compila_host: el constructor en memoria de los NCA del acceso de HOME (app/engine/home_nca.h): hashes, RomFS y registros, sin claves ni consola.
if (Get-Command g++ -ErrorAction SilentlyContinue) {
    Invoke-Stage 'compila_host' 'acceso HOME: constructor de NCA (home_nca, sin claves)' (Join-Path $root 'tools/home_nca_test') {
        g++ -std=c++17 -O1 -Wall -Wextra -I ../../app/engine -o home_nca_test.exe test.cpp
        if ($LASTEXITCODE -ne 0) { throw 'home_nca no compila' }
        ./home_nca_test.exe
        if ($LASTEXITCODE -ne 0) { throw 'home_nca: pruebas fallidas' }
    }
}

$results | Format-Table -AutoSize -Wrap
$failed = @($results | Where-Object { $_.Result -ne 'OK' }).Count
Write-Host ("Stages: {0}, failed: {1}. None of these stages proves the NRO on a console." -f $results.Count, $failed)
exit $(if ($failed -eq 0) { 0 } else { 1 })
