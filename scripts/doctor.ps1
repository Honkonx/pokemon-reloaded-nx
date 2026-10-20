# Comprueba que el entorno de Windows tiene lo necesario para compilar pokenx.nro (ver REQUISITOS.md) y dice qué falta.
# No instala ni modifica nada. Código de salida 1 si falta algo imprescindible.
#
#   pwsh scripts/doctor.ps1
#   pwsh scripts/doctor.ps1 -DevkitPro <carpeta-de-devkitPro>      # si no está en la ubicación estándar
#
# Parámetros opcionales: -MesaSource (por defecto third_party/mesa-switch) y -MesaSdk (por defecto target/mesa26/sdk),
# ambos relativos a la raíz del repositorio.
param(
    [string]$DevkitPro = '',
    [string]$MesaSource = '',
    [string]$MesaSdk = ''
)
$ErrorActionPreference = 'Continue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $MesaSource) { $MesaSource = Join-Path $root 'third_party/mesa-switch' }
if (-not $MesaSdk) { $MesaSdk = Join-Path $root 'target/mesa26/sdk' }

$toolchain = 'nightly-2026-09-29'
$mesaCommit = '2330d007712acbd32e396574203339df39bde153'
$results = New-Object System.Collections.Generic.List[object]

function Add-Result([string]$Group, [string]$Name, [string]$State, [string]$Detail, [string]$Link = '') {
    $results.Add([pscustomobject]@{ Grupo = $Group; Componente = $Name; Estado = $State; Detalle = $Detail; Enlace = $Link })
}
function Test-Tool([string]$Name) { [bool](Get-Command $Name -ErrorAction SilentlyContinue) }
function First-Line($text) { ($text | Out-String).Trim().Split("`n")[0].Trim() }

# --- A. Sistema y herramientas generales ---------------------------------------------------------------------------------
$A = 'A. General'
if ($PSVersionTable.PSVersion.Major -ge 7) { Add-Result $A 'PowerShell 7' 'OK' "$($PSVersionTable.PSVersion)" }
else { Add-Result $A 'PowerShell 7' 'FALTA' "versión $($PSVersionTable.PSVersion); hace falta 7 o posterior" 'https://github.com/PowerShell/PowerShell' }

if (Test-Tool 'git') {
    Add-Result $A 'Git' 'OK' (First-Line (git --version))
    $gitRoot = Split-Path (Split-Path (Get-Command git).Source -Parent) -Parent
    $bash = Join-Path $gitRoot 'bin/bash.exe'
    $standardBash = 'C:\Program Files\Git\bin\bash.exe'
    if (Test-Path $standardBash) { Add-Result $A 'Git Bash (ubicación estándar)' 'OK' 'el script de Mesa la espera en la carpeta estándar de Git' }
    elseif (Test-Path $bash) { Add-Result $A 'Git Bash (ubicación estándar)' 'AVISO' 'Git no está en la carpeta estándar: el script de Mesa usa esa ruta; ajusta $gitBash en scripts/build-mesa-switch.ps1' }
    else { Add-Result $A 'Git Bash' 'FALTA' 'no se encontró bash.exe de Git para Windows' 'https://github.com/git-for-windows/git' }
} else { Add-Result $A 'Git' 'FALTA' 'no está en el PATH' 'https://github.com/git-for-windows/git' }

$pythonOk = $false
if (Test-Tool 'python') {
    $pv = First-Line (python --version 2>&1)
    if ($pv -match 'Python 3\.(\d+)') { $pythonOk = $true; Add-Result $A 'Python 3' 'OK' $pv }
    else { Add-Result $A 'Python 3' 'FALTA' "«$pv»: puede ser el acceso directo de la Tienda de Windows, no un Python real" 'https://www.python.org/downloads/' }
} else { Add-Result $A 'Python 3' 'FALTA' 'no está en el PATH' 'https://www.python.org/downloads/' }

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$msvc = ''
if (Test-Path $vswhere) { $msvc = (& $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Select-Object -First 1) }
if ($msvc) { Add-Result $A 'Build Tools de Visual Studio (C++)' 'OK' 'herramientas de C++ encontradas' }
elseif (Test-Tool 'link') { Add-Result $A 'Build Tools de Visual Studio (C++)' 'OK' 'link.exe en el PATH' }
else { Add-Result $A 'Build Tools de Visual Studio (C++)' 'AVISO' 'no se detectan; Rust en Windows las necesita para sus scripts de compilación y pruebas' 'https://visualstudio.microsoft.com/downloads/' }

if (Test-Tool 'g++') { Add-Result $A 'g++ (MinGW, opcional)' 'OK' (First-Line (g++ --version)) }
else { Add-Result $A 'g++ (MinGW, opcional)' 'OPCIONAL' 'solo para la vista previa del menú en el PC' 'https://github.com/brechtsanders/winlibs_mingw' }

# --- B. Kit de la Switch (devkitPro) --------------------------------------------------------------------------------------
$B = 'B. devkitPro'
if (-not $DevkitPro) {
    # DEVKITPRO suele valer /opt/devkitpro (ruta de MSYS2): solo sirve si es una ruta de Windows existente.
    if ($env:DEVKITPRO -and (Test-Path -LiteralPath $env:DEVKITPRO)) { $DevkitPro = $env:DEVKITPRO }
    else { $DevkitPro = 'C:\devkitPro' }
}
if (Test-Path -LiteralPath $DevkitPro) {
    Add-Result $B 'Carpeta de devkitPro' 'OK' $DevkitPro
    if ($DevkitPro -ne 'C:\devkitPro') { Add-Result $B 'Ubicación' 'AVISO' 'los scripts de compilación usan C:\devkitPro por defecto: edita la variable $devkitPro al principio de app/engine/build.ps1, de scripts/build-mesa-switch.ps1 y de app/engine/tests/check-mesa-abi.ps1' }
    $gxx = Join-Path $DevkitPro 'devkitA64/bin/aarch64-none-elf-g++.exe'
    if (Test-Path $gxx) {
        $gv = First-Line (& $gxx --version 2>&1)
        Add-Result $B 'devkitA64 (GCC)' $(if ($gv -match '16\.1\.') { 'OK' } else { 'AVISO' }) "$gv (verificada: 16.1.0)" 'https://github.com/devkitPro/pacman-packages'
    } else { Add-Result $B 'devkitA64 (GCC)' 'FALTA' 'no existe aarch64-none-elf-g++.exe' 'https://devkitpro.org/wiki/Getting_Started' }
    foreach ($item in @(@('libnx (cabeceras)', 'libnx/include/switch.h'), @('libnx (biblioteca)', 'libnx/lib/libnx.a'),
                         @('elf2nro', 'tools/bin/elf2nro.exe'), @('nacptool', 'tools/bin/nacptool.exe'),
                         @('portlib switch-zlib', 'portlibs/switch/lib/libz.a'), @('portlib switch-libexpat', 'portlibs/switch/lib/libexpat.a'),
                         @('portlib switch-libzstd', 'portlibs/switch/lib/libzstd.a'),
                         @('portlib switch-curl', 'portlibs/switch/lib/libcurl.a'), @('portlib switch-mbedtls', 'portlibs/switch/lib/libmbedtls.a'))) {
        if (Test-Path (Join-Path $DevkitPro $item[1])) { Add-Result $B $item[0] 'OK' $item[1] }
        else { Add-Result $B $item[0] 'FALTA' "no existe $($item[1])" 'https://github.com/switchbrew/libnx' }
    }
} else { Add-Result $B 'devkitPro' 'FALTA' "no se encontró ($DevkitPro); usa -DevkitPro <carpeta> si está en otro sitio" 'https://github.com/devkitPro/installer' }

# --- C. Rust ---------------------------------------------------------------------------------------------------------------------
$C = 'C. Rust'
if (Test-Tool 'rustup') {
    Add-Result $C 'rustup' 'OK' (First-Line (rustup --version 2>&1))
    $installed = (rustup toolchain list 2>&1 | Out-String)
    if ($installed -match [regex]::Escape($toolchain)) {
        Add-Result $C "toolchain $toolchain" 'OK' (First-Line (cargo "+$toolchain" --version 2>&1))
        $components = (rustup component list --toolchain $toolchain --installed 2>&1 | Out-String)
        if ($components -match 'rust-src') { Add-Result $C 'componente rust-src' 'OK' 'instalado' } else { Add-Result $C 'componente rust-src' 'FALTA' "rustup component add rust-src --toolchain $toolchain" }
        if ($components -match 'aarch64-unknown-linux-gnu') { Add-Result $C 'target aarch64-unknown-linux-gnu' 'OK' 'instalado' }
        else {
            $targets = (rustup target list --toolchain $toolchain --installed 2>&1 | Out-String)
            if ($targets -match 'aarch64-unknown-linux-gnu') { Add-Result $C 'target aarch64-unknown-linux-gnu' 'OK' 'instalado' }
            else { Add-Result $C 'target aarch64-unknown-linux-gnu' 'FALTA' "rustup target add aarch64-unknown-linux-gnu --toolchain $toolchain (solo hace falta para compilar Mesa)" }
        }
    } else { Add-Result $C "toolchain $toolchain" 'FALTA' "rustup toolchain install $toolchain --profile minimal -c rust-src -t aarch64-unknown-linux-gnu" 'https://rust-lang.github.io/rustup/' }
} else { Add-Result $C 'rustup' 'FALTA' 'no está en el PATH' 'https://github.com/rust-lang/rustup' }

# --- D. Mesa: ¿ya hay SDK compilado? Si sí, el resto de herramientas de Mesa no hacen falta --------------------------------------
$D = 'D. Mesa'
$sdkReady = $false
$manifestPath = Join-Path $MesaSdk 'pokenx-mesa-sdk.json'
if (Test-Path $manifestPath) {
    try {
        $manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json
        $bad = @()
        foreach ($prop in $manifest.archives.PSObject.Properties) {
            $file = Join-Path $MesaSdk "lib/$($prop.Name)"
            if (-not (Test-Path $file)) { $bad += "$($prop.Name) (falta)"; continue }
            if ((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash -ne $prop.Value) { $bad += "$($prop.Name) (hash distinto)" }
        }
        if ($manifest.source_commit -ne $mesaCommit) { $bad += 'commit de origen distinto' }
        if ($bad.Count -eq 0) { $sdkReady = $true; Add-Result $D 'SDK de Mesa compilado' 'OK' "Mesa $($manifest.mesa_version), hashes verificados" }
        else { Add-Result $D 'SDK de Mesa compilado' 'FALTA' ("incompleto o distinto: " + ($bad -join ', ') + '. Vuelve a ejecutar scripts/build-mesa-switch.ps1 -Stage Install') }
    } catch { Add-Result $D 'SDK de Mesa compilado' 'FALTA' "manifiesto ilegible: $($_.Exception.Message)" }
} else { Add-Result $D 'SDK de Mesa compilado' 'FALTA' 'no existe target/mesa26/sdk: hay que compilarlo (docs/GUIA.md, pasos 5 a 7)' 'https://github.com/NaGaa95/mesa-switch' }

if ($sdkReady) {
    Add-Result $D 'Herramientas para compilar Mesa' 'OPCIONAL' 'no hacen falta: el SDK ya está compilado'
} else {
    $mesaRepoOk = $false
    if (Test-Path (Join-Path $MesaSource '.git')) {
        $head = (git -C $MesaSource rev-parse HEAD 2>$null | Out-String).Trim()
        if ($head -eq $mesaCommit) { $mesaRepoOk = $true; Add-Result $D 'Código de Mesa (commit fijado)' 'OK' $mesaCommit.Substring(0, 8) }
        else { Add-Result $D 'Código de Mesa (commit fijado)' 'FALTA' "está en $($head.Substring(0, [Math]::Min(8, $head.Length))); git -C third_party/mesa-switch checkout $mesaCommit" }
    } else { Add-Result $D 'Código de Mesa (commit fijado)' 'FALTA' 'git clone https://github.com/NaGaa95/mesa-switch third_party/mesa-switch' 'https://github.com/NaGaa95/mesa-switch' }

    $tools = Join-Path $root 'target/mesa-tools'
    $meson = Join-Path $tools 'venv/Scripts/meson.exe'
    if (Test-Path $meson) { Add-Result $D 'meson (entorno de Python)' 'OK' (First-Line (& $meson --version 2>&1)) } else { Add-Result $D 'meson (entorno de Python)' 'FALTA' 'python -m venv target\mesa-tools\venv; …\pip install -r scripts\mesa\requirements.txt' 'https://github.com/mesonbuild/meson' }
    $bindgen = Join-Path $tools 'cargo/bin/bindgen.exe'
    if (Test-Path $bindgen) { $bv = First-Line (& $bindgen --version 2>&1); Add-Result $D 'bindgen-cli' $(if ($bv -match '0\.72\.1') { 'OK' } else { 'AVISO' }) "$bv (verificada: 0.72.1)" 'https://github.com/rust-lang/rust-bindgen' }
    else { Add-Result $D 'bindgen-cli' 'FALTA' "cargo +$toolchain install bindgen-cli --version 0.72.1 --locked --root target\mesa-tools\cargo" 'https://github.com/rust-lang/rust-bindgen' }
    if ($DevkitPro -and (Test-Path -LiteralPath $DevkitPro)) {
        $clang = Join-Path $DevkitPro 'msys2/clang64/bin'
        foreach ($exe in 'clang.exe', 'llvm-config.exe', 'ninja.exe', 'cmake.exe', 'cbindgen.exe') {
            if (Test-Path (Join-Path $clang $exe)) { Add-Result $D "clang64: $exe" 'OK' 'encontrado' } else { Add-Result $D "clang64: $exe" 'FALTA' 'instálalo con pacman en el MSYS2 de devkitPro (docs/GUIA.md, paso 5)' 'https://www.msys2.org/' }
        }
    }
    if (Test-Path (Join-Path $tools 'pacman.conf')) { Add-Result $D 'pacman.conf con [clang64]' $(if ((Get-Content (Join-Path $tools 'pacman.conf') -Raw) -match '\[clang64\]') { 'OK' } else { 'FALTA' }) 'target/mesa-tools/pacman.conf' }
    else { Add-Result $D 'pacman.conf con [clang64]' 'FALTA' 'copia el de devkitPro a target\mesa-tools y añade la sección [clang64] (docs/GUIA.md, paso 5)' }
}

# --- Resultado -----------------------------------------------------------------------------------------------------------------------
$colors = @{ OK = 'Green'; FALTA = 'Red'; AVISO = 'Yellow'; OPCIONAL = 'DarkGray' }
$group = ''
foreach ($r in $results) {
    if ($r.Grupo -ne $group) { $group = $r.Grupo; Write-Host ""; Write-Host $group -ForegroundColor Cyan }
    Write-Host ("  [{0,-8}] {1,-40} {2}" -f $r.Estado, $r.Componente, $r.Detalle) -ForegroundColor $colors[$r.Estado]
    if ($r.Estado -in 'FALTA', 'AVISO' -and $r.Enlace) { Write-Host ("             {0}" -f $r.Enlace) -ForegroundColor DarkGray }
}
$missing = @($results | Where-Object { $_.Estado -eq 'FALTA' }).Count
$warn = @($results | Where-Object { $_.Estado -eq 'AVISO' }).Count
Write-Host ""
Write-Host ("Faltan: {0}. Avisos: {1}. Detalle de cada herramienta: REQUISITOS.md; pasos: docs/GUIA.md; errores: docs/ERRORES-FRECUENTES.md." -f $missing, $warn)
if ($missing -eq 0) { Write-Host 'El entorno parece completo. Esto no prueba que el NRO compile ni arranque.' }
exit $(if ($missing -eq 0) { 0 } else { 1 })
