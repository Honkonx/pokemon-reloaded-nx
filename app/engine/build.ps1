# Builds the PokeNX engine payload (libnx C++ entrypoint + Rust staticlib).
# The resulting NRO is an INTERNAL artifact: app/build-package.ps1 embeds it in
# pokenx.nro, the only distributable file. Build success does not prove it runs.
param([string]$MesaSdk = (Join-Path $PSScriptRoot '../../target/mesa26/sdk'))
$ErrorActionPreference = 'Stop'

$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$devkitPro = 'C:\devkitPro'
$compiler = Join-Path $devkitPro 'devkitA64\bin\aarch64-none-elf-g++.exe'
$readelf = Join-Path $devkitPro 'devkitA64\bin\aarch64-none-elf-readelf.exe'
$elf2nro = Join-Path $devkitPro 'tools\bin\elf2nro.exe'
$nacptool = Join-Path $devkitPro 'tools\bin\nacptool.exe'
$portlibs = Join-Path $devkitPro 'portlibs\switch'
$libnx = Join-Path $devkitPro 'libnx'
$engineCrate = Join-Path $root 'src\gm8-runtime\pokenx-engine'
$archive = Join-Path $engineCrate 'target\aarch64-nintendo-switch-freestanding\release\libpokenx_engine.a'
$output = Join-Path $PSScriptRoot 'target'
$elf = Join-Path $output 'pokenx-engine.elf'
$nacp = Join-Path $output 'pokenx-engine.nacp'
$nro = Join-Path $output 'pokenx.nro'
$icon = Join-Path $root 'app\assets\pokenx-icon-256.jpg'

New-Item -ItemType Directory -Force -Path $output | Out-Null

$graphicsIncludes = @()
$graphicsLibraries = @('-lEGL', '-lglapi', '-ldrm_nouveau')
$graphicsDefines = @()
$graphicsSources = @()
$graphicsBuild = [ordered]@{ mesa = '20.1.0'; unified = $false; source_commit = ''; sdk_manifest_hash = '' }
if ($MesaSdk) {
    $MesaSdk = (Resolve-Path -LiteralPath $MesaSdk).Path
    $manifestPath = Join-Path $MesaSdk 'pokenx-mesa-sdk.json'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.schema -ne 1 -or $manifest.target -ne 'aarch64-horizon' -or
        $manifest.source_commit -ne '2330d007712acbd32e396574203339df39bde153' -or
        'zink' -notin $manifest.gallium_drivers -or 'nouveau' -notin $manifest.vulkan_drivers) {
        throw 'Unsupported or incomplete Mesa SDK manifest'
    }
    $graphicsIncludes = @("-I$(Join-Path $MesaSdk 'include')")
    $graphicsLibraries = @()
    foreach ($name in 'EGL', 'glapi', 'vulkan', 'mesa_util_c11', 'blake3', 'mesa_util', 'mesa_util_simd', 'xmlconfig') {
        $archiveName = "lib$name.a"
        $file = Join-Path $MesaSdk "lib/$archiveName"
        if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $manifest.archives.$archiveName) {
            throw "Mesa SDK archive differs from its build manifest: $file"
        }
        $graphicsLibraries += $file
    }
    $graphicsLibraries += @('-lexpat', '-lzstd', '-lz')
    $graphicsLibraries += '-Wl,--wrap=pthread_create'
    $graphicsDefines = @('-DPOKENX_MESA26=1')
    $graphicsSources = @((Join-Path $PSScriptRoot 'mesa_linux_compat.cpp'))
    $graphicsBuild = [ordered]@{ mesa = $manifest.mesa_version; unified = $true;
        source_commit = $manifest.source_commit; sdk_manifest_hash = (Get-FileHash $manifestPath -Algorithm SHA256).Hash }
}

# The Switch's CPU is a Cortex-A57; the freestanding target defaults to generic AArch64, so tell LLVM to schedule for it.
$previousRustflags = $env:RUSTFLAGS
$env:RUSTFLAGS = '-C target-cpu=cortex-a57'
cargo +nightly-2026-09-29 build --release --manifest-path (Join-Path $engineCrate 'Cargo.toml') `
    --no-default-features --features switch --target aarch64-nintendo-switch-freestanding `
    -Z build-std=core,alloc,compiler_builtins
$env:RUSTFLAGS = $previousRustflags
if ($LASTEXITCODE -ne 0) { throw "Rust engine staticlib build failed: $LASTEXITCODE" }

if ($MesaSdk) {
    $rustVersion = (& rustc +nightly-2026-09-29 --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $rustVersion -ne $manifest.rustc_version) {
        throw 'Mesa and engine Rust compiler versions must match before sharing panic entry points'
    }
    # Rust staticlibs carry their own panic/allocation failure entry points.
    # Make ONLY those three Mesa definitions weak in a build-local archive;
    # the engine retains its strong abort handlers. Never hide other conflicts.
    $nm = Join-Path $devkitPro 'devkitA64/bin/aarch64-none-elf-nm.exe'
    $objcopy = Join-Path $devkitPro 'devkitA64/bin/aarch64-none-elf-objcopy.exe'
    $rawVulkan = Join-Path $MesaSdk 'lib/libvulkan.a'
    $symbols = & $nm --defined-only --format=posix $rawVulkan
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect Mesa Rust definitions' }
    $panicPattern = '^(_RNv\w+_7___rustc(?:17rust_begin_unwind|26___rust_alloc_error_handler|25___rdl_alloc_error_handler)) T '
    $shared = @($symbols | ForEach-Object { if ($_ -match $panicPattern) { $Matches[1] } } | Sort-Object -Unique)
    if ($shared.Count -ne 3) { throw 'Unexpected Mesa Rust panic ABI; review composition explicitly' }
    $engineSymbols = & $nm --defined-only --format=posix $archive
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect engine Rust definitions' }
    foreach ($symbol in $shared) {
        if (-not ($engineSymbols | Where-Object { $_ -match ('^' + [regex]::Escape($symbol) + ' T ') })) {
            throw "Engine does not own the shared Rust entry point: $symbol"
        }
    }
    $localVulkan = Join-Path $output 'libvulkan-engine-panic.a'
    $weakArgs = @($shared | ForEach-Object { '--symbol'; $_ })
    # The Mesa build's venv python when it exists (it is created by scripts/build-mesa-switch.ps1), else python on PATH.
    $composePython = Join-Path $root 'target/mesa-tools/venv/Scripts/python.exe'
    if (-not (Test-Path -LiteralPath $composePython)) { $composePython = (Get-Command python -ErrorAction Stop).Source }
    & $composePython `
        (Join-Path $root 'scripts/mesa/compose_rust_archive.py') $rawVulkan $localVulkan `
        --objcopy $objcopy --ar (Join-Path $devkitPro 'devkitA64/bin/aarch64-none-elf-ar.exe') @weakArgs
    if ($LASTEXITCODE -ne 0) { throw 'Cannot prepare Mesa archive with engine-owned panic handlers' }
    $localSymbols = & $nm --defined-only --format=posix $localVulkan
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect composed Mesa symbols' }
    foreach ($symbol in $shared) {
        $matching = @($localSymbols | Where-Object { $_ -match ('^' + [regex]::Escape($symbol) + ' ') })
        if (-not $matching -or @($matching | Where-Object { $_ -notmatch ('^' + [regex]::Escape($symbol) + ' W ') }).Count) {
            throw "Mesa entry point was not made weak: $symbol"
        }
    }
    $graphicsLibraries = @($graphicsLibraries | ForEach-Object { if ($_ -eq $rawVulkan) { $localVulkan } else { $_ } })
    $graphicsBuild.shared_rust_entry_points = $shared
    $graphicsBuild.link_vulkan_sha256 = (Get-FileHash $localVulkan -Algorithm SHA256).Hash
    $graphicsBuild.rust_native_bridge = 'pokenx-private-libc-v1'
}

$env:DEVKITPRO = $devkitPro
$env:DEVKITA64 = Join-Path $devkitPro 'devkitA64'

# The HOME shortcut's program (app/forwarder: a patched nx-hbloader) travels inside the NRO so the menu can install it
# (home_install.cpp). Build it when missing or older than its sources, then turn its ExeFS (a PFS0 in pokenx-forwarder.nsp)
# into a C++ array.
$forwarderDir = Join-Path $root 'app/forwarder'
$forwarderNsp = Join-Path $forwarderDir 'pokenx-forwarder.nsp'
$forwarderInputs = @(Get-ChildItem -LiteralPath (Join-Path $forwarderDir 'source') -File) + @(Get-Item -LiteralPath (Join-Path $forwarderDir 'Makefile'), (Join-Path $forwarderDir 'forwarder.json'))
if (-not (Test-Path -LiteralPath $forwarderNsp) -or ($forwarderInputs | Where-Object { $_.LastWriteTime -gt (Get-Item -LiteralPath $forwarderNsp).LastWriteTime })) {
    & (Join-Path $forwarderDir 'build.ps1') -DevkitPro $devkitPro | Out-Null
}
$forwarderBytes = [IO.File]::ReadAllBytes($forwarderNsp)
if ([Text.Encoding]::ASCII.GetString($forwarderBytes, 0, 4) -ne 'PFS0') { throw 'pokenx-forwarder.nsp is not a PFS0' }
$forwarderBlob = Join-Path $output 'forwarder_blob.cpp'
$forwarderHex = [BitConverter]::ToString($forwarderBytes).Replace('-', ',0x')
Set-Content -LiteralPath $forwarderBlob -Encoding ascii -Value "extern const unsigned char kForwarderExefs[] = {0x$forwarderHex};`nextern const unsigned int kForwarderExefsSize = $($forwarderBytes.Length);`n"
# Link with our own copy of libnx's linker script (switch-pokenx.ld): it moves .rodata into the writable RELRO area.
# The stock script keeps .rodata read-only, but the non-PIC libstdc++/libc objects (and our pointer tables) carry
# relocations there; libnx's crt0 writes them at startup, hbloader maps that segment read-only, and the NRO then dies
# before userAppInit (no log, black screen). -z notext only silences the linker's complaint about that; the check after
# the link (scripts/verify_relocations.py) is what guarantees every relocation is in a writable segment.
$ldScript = (Join-Path $PSScriptRoot 'switch-pokenx.ld').Replace('\', '/')
$specsFile = Join-Path $output 'switch-pokenx.specs'
(Get-Content -LiteralPath (Join-Path $libnx 'switch.specs') -Raw).Replace('%:getenv(DEVKITPRO /libnx/switch.ld)', $ldScript) |
    Set-Content -LiteralPath $specsFile -Encoding ascii -NoNewline
& $compiler "-specs=$specsFile" -O2 -mtune=cortex-a57 -D__SWITCH__ -std=gnu++17 -mtp=soft `
    @graphicsIncludes @graphicsDefines "-I$(Join-Path $libnx 'include')" "-I$(Join-Path $portlibs 'include')" "-I$PSScriptRoot" `
    (Join-Path $PSScriptRoot 'main.cpp') (Join-Path $PSScriptRoot 'host_services.cpp') (Join-Path $PSScriptRoot 'menu.cpp') (Join-Path $PSScriptRoot 'menu_text.cpp') (Join-Path $PSScriptRoot 'gfx.cpp') (Join-Path $PSScriptRoot 'updater.cpp') (Join-Path $PSScriptRoot 'home_install.cpp') $forwarderBlob @graphicsSources $archive `
    "-L$(Join-Path $portlibs 'lib')" "-L$(Join-Path $libnx 'lib')" `
    '-Wl,--start-group' @graphicsLibraries -lcurl -lmbedtls -lmbedx509 -lmbedcrypto -lz -lnx -lm '-Wl,--end-group' `
    '-Wl,-z,notext' "-Wl,-Map=$output\pokenx-engine.map" -o $elf
if ($LASTEXITCODE -ne 0) { throw "Engine link failed: $LASTEXITCODE" }

$python = Join-Path $root 'target/mesa-tools/venv/Scripts/python.exe'
if (-not (Test-Path -LiteralPath $python)) { $python = (Get-Command python -ErrorAction Stop).Source }
& $python (Join-Path $root 'scripts/verify_relocations.py') $elf
if ($LASTEXITCODE -ne 0) { throw 'Relocation check failed: the NRO would crash at startup (see scripts/verify_relocations.py)' }

& $readelf -h $elf | Select-String 'Class:|Machine:'
& $nacptool --create 'PokeNX' 'PokeNX Project' '0.1.0' $nacp --titleid=0100000000000F04
if ($LASTEXITCODE -ne 0) { throw "NACP creation failed: $LASTEXITCODE" }
& $elf2nro $elf $nro "--nacp=$nacp" "--icon=$icon"
if ($LASTEXITCODE -ne 0) { throw "NRO packaging failed: $LASTEXITCODE" }
$graphicsBuild | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'graphics-build.json') -Encoding utf8
Get-Item $archive, $elf, $nro | Select-Object FullName, Length
Write-Host 'Single PokeNX NRO built (menu + engine). Not proven to run until tested on a Switch.'
