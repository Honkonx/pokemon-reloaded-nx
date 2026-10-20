# Build the pinned Mesa Switch reference in isolation; never replaces devkitPro portlibs.
param(
    [ValidateSet('ConfigureHost', 'BuildHost', 'Configure', 'Build', 'Install', 'All')]
    [string]$Stage = 'All',
    [ValidateRange(1, 32)][int]$Jobs = 4,
    [switch]$ClearCache
)
$ErrorActionPreference = 'Stop'
$taskRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path.Replace('\', '/')
$sourceRoot = "$taskRoot/third_party/mesa-switch"
$toolsRoot = "$taskRoot/target/mesa-tools"
$buildRoot = "$taskRoot/target/mesa26"
$sdkRoot = 'C:/devkitPro'
$msysRoot = "$sdkRoot/msys2"
$clangRoot = "$msysRoot/clang64"
$expectedCommit = '2330d007712acbd32e396574203339df39bde153'
$gitBash = 'C:/Program Files/Git/bin/bash.exe'
$actualCommit = & $gitBash -c 'git -C "$1" rev-parse HEAD' -- $sourceRoot
if ($LASTEXITCODE -ne 0 -or $actualCommit.Trim() -ne $expectedCommit) {
    throw "Review Mesa provenance before building a different checkout: $actualCommit"
}
$bash = "$msysRoot/usr/bin/bash.exe"
$meson = "$toolsRoot/venv/Scripts/meson.exe"
$ninja = "$clangRoot/bin/ninja.exe"
$rustc = (& rustup which --toolchain nightly-2026-09-29 rustc).Replace('\', '/')
if ($LASTEXITCODE -ne 0) { throw 'Pinned Rust toolchain missing' }
foreach ($required in @($meson, $ninja, "$clangRoot/bin/clang.exe", "$clangRoot/bin/llvm-config.exe",
    "$toolsRoot/cargo/bin/bindgen.exe", "$sourceRoot/src/compiler/clc/meson.build", "$sdkRoot/libnx/lib/libnx.a")) {
    if (-not (Test-Path -LiteralPath $required)) { throw "Required file missing: $required; see scripts/mesa/README.md" }
}
New-Item -ItemType Directory -Force -Path "$buildRoot/wrappers" | Out-Null
$utf8 = [System.Text.UTF8Encoding]::new($false)
function Write-Utf8($path, $content) {
    $normalized = $content.Replace("`r`n", "`n")
    if ((Test-Path -LiteralPath $path) -and [IO.File]::ReadAllText($path) -ceq $normalized) { return }
    [IO.File]::WriteAllText($path, $normalized, $utf8)
}

# Adapt the upstream MSYS2 files to this devkitPro layout without editing references.
foreach ($name in 'switch-pkg-config-wrapper.sh', 'bindgen-switch-wrapper.sh', 'rustc-switch-wrapper.sh', 'rustc-native-msys2-wrapper.sh') {
    $content = Get-Content -LiteralPath "$sourceRoot/$name" -Raw
    $content = $content.Replace('C:/msys64/opt/devkitpro', $sdkRoot).Replace('C:/msys64', $msysRoot)
    if ($name -eq 'bindgen-switch-wrapper.sh') {
        $content = $content.Replace('SHIM=$SCRIPT_DIR/bindgen-atomic-shim.h', "SHIM='$sourceRoot/bindgen-atomic-shim.h'")
        $content = $content.Replace('SWITCH_SYS_INCLUDE=${DEVKITPRO:-/opt/devkitpro}/devkitA64/aarch64-none-elf/include',
            "SWITCH_SYS_INCLUDE='$sdkRoot/devkitA64/aarch64-none-elf/include'")
        $content = $content.Replace('SWITCH_LIBNX_INCLUDE=${DEVKITPRO:-/opt/devkitpro}/libnx/include',
            "SWITCH_LIBNX_INCLUDE='$sdkRoot/libnx/include'")
    }
    if ($name -eq 'rustc-switch-wrapper.sh') {
        $content = $content.Replace('*sanitycheckrs.rs*)', '*sanitycheckrs.rs*|*sanity.rs*)')
        # This machine's Rust host is MSVC. Its native sanity check must use
        # rustc's MSVC linker discovery, not upstream's GNU-host fallback.
        $marker = 'FALLBACK_LINKER='
        $offset = $content.IndexOf($marker)
        if ($offset -lt 0) { throw 'Upstream Rust wrapper changed; review before adapting' }
        $content = $content.Substring(0, $offset) + 'exec "$RUSTC_BIN" "${ARGS[@]}"' + "`n"
    }
    if ($name -eq 'rustc-native-msys2-wrapper.sh') {
        $content = $content.Replace('exec "$RUSTC_BIN" -C linker=C:/devkitPro/msys2/ucrt64/bin/gcc.exe "${ARGS[@]}"',
            'exec "$RUSTC_BIN" "${ARGS[@]}"')
    }
    Write-Utf8 "$buildRoot/wrappers/$name" $content
}
$cross = Get-Content "$sourceRoot/switch_cross_file_msys2.txt" -Raw
$cross = $cross.Replace('C:/msys64/opt/devkitpro', $sdkRoot).Replace('C:/msys64', $msysRoot)
$cross = $cross.Replace('@DIRNAME@', "$buildRoot/wrappers")
Write-Utf8 "$buildRoot/switch.ini" $cross
$hostCross = (Get-Content "$sourceRoot/windows_x64_host_cross_file_msys2.txt" -Raw).Replace('C:/msys64', $msysRoot)
Write-Utf8 "$buildRoot/host.ini" $hostCross
$native = @"
[binaries]
c = '$clangRoot/bin/clang.exe'
cpp = '$clangRoot/bin/clang++.exe'
ar = '$clangRoot/bin/llvm-ar.exe'
rust = ['$bash', '$buildRoot/wrappers/rustc-native-msys2-wrapper.sh']
mesa_clc = '$buildRoot/host/src/compiler/clc/mesa_clc.exe'
vtn_bindgen2 = '$buildRoot/host/src/compiler/spirv/vtn_bindgen2.exe'
cbindgen = '$clangRoot/bin/cbindgen.exe'
"@
Write-Utf8 "$buildRoot/native.ini" $native
$names = @('PATH', 'MESA_SWITCH_RUSTC', 'MESA_SWITCH_NATIVE_RUSTC', 'MESA_SWITCH_RUST_TARGET', 'MESA_SWITCH_BINDGEN', 'LIBCLANG_PATH', 'NINJA', 'PYTHONUTF8')
$previous = @{}
foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
function Invoke-Meson([string[]]$Arguments) {
    & $meson @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Meson failed: $LASTEXITCODE" }
}
function Configure-Host {
    $mode = @()
    if (Test-Path "$buildRoot/host/meson-private/coredata.dat") { $mode = @('--reconfigure') }
    Invoke-Meson (@('setup', "$buildRoot/host", $sourceRoot) + $mode + @('--cross-file', "$buildRoot/host.ini",
        '--buildtype=release', '-Dvulkan-drivers=', '-Dgallium-drivers=', '-Dshader-cache=disabled', '-Dplatforms=',
        '-Dglx=disabled', '-Degl=disabled', '-Dopengl=false', '-Dgles1=disabled', '-Dgles2=disabled', '-Dtools=[]',
        '-Dllvm=enabled', '-Dmesa-clc=enabled', '-Dprecomp-compiler=enabled', '-Dinstall-mesa-clc=true'))
}
function Build-Host {
    & $ninja -C "$buildRoot/host" -j $Jobs src/compiler/clc/mesa_clc.exe src/compiler/spirv/vtn_bindgen2.exe
    if ($LASTEXITCODE -ne 0) { throw "Native shader tools build failed: $LASTEXITCODE" }
}
function Configure-Switch {
    $mode = @()
    if (Test-Path "$buildRoot/switch/meson-private/coredata.dat") { $mode = @('--reconfigure') }
    if ($ClearCache) { $mode += '--clearcache' }
    Invoke-Meson (@('setup', "$buildRoot/switch", $sourceRoot) + $mode + @('--cross-file', "$buildRoot/switch.ini",
        '--native-file', "$buildRoot/native.ini", '--default-library=static', "--prefix=$buildRoot/sdk",
        '--libdir=lib', '--buildtype=release', '-Doptimization=2', '-Db_lto=false', '-Db_ndebug=true',
        '-Dvulkan-drivers=nouveau', '-Dgallium-drivers=nouveau,zink', '-Dgallium-rusticl=false',
        '-Dplatforms=switch', '-Degl-native-platform=switch', '-Dglx=disabled', '-Degl=enabled', '-Dopengl=true',
        '-Dgles1=enabled', '-Dgles2=enabled', '-Dvideo-codecs=', '-Dshader-cache=enabled', '-Dxmlconfig=enabled',
        '-Dexpat=enabled', '-Dtools=[]', '-Dllvm=disabled', '-Dshared-glapi=disabled', '-Dshared-llvm=disabled',
        '-Dmesa-clc=system', '-Dprecomp-compiler=system', '-Dcpp_rtti=false', '-Dbuild-tests=false',
        "-Dnvk-build-id=$expectedCommit"))
}
try {
    # Prevent a globally installed win_flex from adding Windows-only io.h to
    # Horizon lexers. Every required native compiler is selected explicitly.
    $rustBin = Split-Path $rustc
    $env:PATH = "$toolsRoot/venv/Scripts;$clangRoot/bin;$msysRoot/usr/bin;$rustBin;C:/Program Files/Git/cmd;$env:SystemRoot/System32;$env:SystemRoot"
    $env:PYTHONUTF8 = '1'
    $env:MESA_SWITCH_RUSTC = $rustc
    $env:MESA_SWITCH_NATIVE_RUSTC = $rustc
    $env:MESA_SWITCH_RUST_TARGET = 'aarch64-unknown-linux-gnu'
    $env:MESA_SWITCH_BINDGEN = "$toolsRoot/cargo/bin/bindgen.exe"
    $env:LIBCLANG_PATH = "$clangRoot/bin"
    $env:NINJA = $ninja
    if ($Stage -in 'ConfigureHost', 'All') { Configure-Host }
    if ($Stage -in 'BuildHost', 'All') { Build-Host }
    if ($Stage -in 'Configure', 'All') { Configure-Switch }
    if ($Stage -in 'Build', 'All') {
        & $ninja -C "$buildRoot/switch" -j $Jobs
        if ($LASTEXITCODE -ne 0) { throw "Switch Mesa build failed: $LASTEXITCODE" }
    }
    if ($Stage -in 'Install', 'All') {
        Invoke-Meson -Arguments @('install', '-C', "$buildRoot/switch")
        $archives = @{}
        foreach ($name in 'EGL', 'glapi', 'vulkan', 'mesa_util_c11', 'blake3', 'mesa_util', 'mesa_util_simd', 'xmlconfig') {
            $file = "$buildRoot/sdk/lib/lib$name.a"
            if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "SDK archive missing: $file" }
            $headers = & "$sdkRoot/devkitA64/bin/aarch64-none-elf-readelf.exe" -h $file 2>&1 | Out-String
            $machines = @([regex]::Matches($headers, 'Machine:[ \t]*(\S+)'))
            if ($LASTEXITCODE -ne 0 -or $machines.Count -eq 0 -or
                @($machines | Where-Object { $_.Groups[1].Value -ne 'AArch64' }).Count -gt 0) {
                throw "SDK archive is not exclusively AArch64 ELF: $file"
            }
            $archives["lib$name.a"] = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash
        }
        $manifest = [ordered]@{ schema = 1; source_commit = $expectedCommit;
            source_repository = 'https://github.com/NaGaa95/mesa-switch'; mesa_version = '26.2.4';
            rustc_version = (& $rustc --version | Out-String).Trim();
            target = 'aarch64-horizon'; gallium_drivers = @('nouveau', 'zink'); vulkan_drivers = @('nouveau');
            evidence = 'compiled_not_executed'; archives = $archives }
        Write-Utf8 "$buildRoot/sdk/pokenx-mesa-sdk.json" ($manifest | ConvertTo-Json -Depth 4)
    }
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
