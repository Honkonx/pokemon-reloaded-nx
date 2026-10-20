# Builds the single distributable, pokenx.nro (graphical menu + engine + Mesa in one binary), and CREATES the SD folder
# `pokenx/` from scratch (nothing of it is versioned). Copying pokenx.nro (or the whole pokenx/ folder) to sdmc:/switch/
# is the whole install. Build success is not evidence that anything runs on the console.
#
# Requirements: devkitPro, Rust nightly-2026-09-29 and the Mesa SDK built with scripts/build-mesa-switch.ps1 into
# target/mesa26/sdk (its source is cloned in third_party/mesa-switch).
param(
    [string]$MesaSdk = (Join-Path $PSScriptRoot '../target/mesa26/sdk'),
    [string]$MesaSource = (Join-Path $PSScriptRoot '../third_party/mesa-switch'),  # for the license texts of the package
    [string]$SoundFont = ''   # optional General MIDI SoundFont copied next to the NRO (MIDI music is silent without one)
)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$packageRoot = Join-Path $projectRoot 'pokenx'
$builtNro = Join-Path $PSScriptRoot 'engine\target\pokenx.nro'

# Never package a locally installed game: only empty placeholders are allowed under pokenx/game.
$gameRoot = Join-Path $packageRoot 'game'
if (Test-Path -LiteralPath $gameRoot -PathType Container) {
    $unexpected = Get-ChildItem -LiteralPath $gameRoot -File -Force -Recurse | Where-Object { $_.Name -ne '.gitkeep' }
    if ($unexpected) { throw "Refusing to package local game files under ${gameRoot}: $(($unexpected | Select-Object -ExpandProperty FullName) -join ', ')" }
}

# 1. Build the single NRO.
& (Join-Path $PSScriptRoot 'engine\build.ps1') -MesaSdk $MesaSdk
if ($LASTEXITCODE -ne 0) { throw "Engine build failed with exit code $LASTEXITCODE" }
if (-not (Test-Path -LiteralPath $builtNro -PathType Leaf)) { throw "Build output not found: $builtNro" }
$graphicsBuild = Get-Content (Join-Path $PSScriptRoot 'engine/target/graphics-build.json') -Raw | ConvertFrom-Json

# 2. SD layout: one NRO plus the data folders the program expects.
foreach ($directory in @('game', 'game\data', 'logs', 'shader', 'cache', 'saves')) {
    New-Item -ItemType Directory -Force -Path (Join-Path $packageRoot $directory) | Out-Null
}
Copy-Item -LiteralPath $builtNro -Destination (Join-Path $packageRoot 'pokenx.nro') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'engine/target/graphics-build.json') -Destination (Join-Path $packageRoot 'graphics-build.json') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets\pokenx-cover.png') -Destination (Join-Path $packageRoot 'cover.png') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets\pokenx-icon.png') -Destination (Join-Path $packageRoot 'icon.png') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets\graphics-driver.txt') -Destination (Join-Path $packageRoot 'graphics-driver.txt') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'assets\cacert.pem') -Destination (Join-Path $packageRoot 'cacert.pem') -Force  # lets the menu verify HTTPS for game updates

# 3. License texts of the Mesa build that is linked into the NRO (from the Mesa source clone) and the project notice.
if ($graphicsBuild.unified) {
    $noticeDir = Join-Path $packageRoot 'licenses/mesa26'
    New-Item -ItemType Directory -Force -Path $noticeDir | Out-Null
    $mesaLicenses = Join-Path $MesaSource 'licenses'
    if (Test-Path -LiteralPath $mesaLicenses -PathType Container) {
        Copy-Item -LiteralPath $mesaLicenses -Destination $noticeDir -Recurse -Force
        Copy-Item -LiteralPath (Join-Path $MesaSource 'docs/license.rst') -Destination (Join-Path $noticeDir 'license.rst') -Force
    } else {
        Write-Warning "Mesa source not found at ${MesaSource}: the Mesa license texts were NOT copied. Add them before redistributing the package."
    }
    $projectNotice = Join-Path $projectRoot 'NOTICE-mesa26.md'  # optional: the Mesa texts above are what the package needs
    if (Test-Path -LiteralPath $projectNotice -PathType Leaf) { Copy-Item -LiteralPath $projectNotice -Destination (Join-Path $noticeDir 'NOTICE.md') -Force }
}

# 4. Optional SoundFont (not part of the program) with its license.
if ($SoundFont) {
    if (-not (Test-Path -LiteralPath $SoundFont -PathType Leaf)) { throw "SoundFont not found: $SoundFont" }
    Copy-Item -LiteralPath $SoundFont -Destination (Join-Path $packageRoot 'soundfont.sf2') -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'src\gm8-runtime\LICENCE.md') -Destination (Join-Path $packageRoot 'SOUNDFONT-LICENSE.txt') -Force
    Write-Host 'SoundFont copied to the package (MIDI music). Keep SOUNDFONT-LICENSE.txt with it in any redistribution.'
} else {
    Write-Host 'No SoundFont given: MIDI music stays silent until soundfont.sf2 is placed in sdmc:/switch/pokenx/.'
}

# 5. A short README inside the package (generated here so the folder never depends on a versioned template).
@'
PokeNX - native OpenGMK runtime for Nintendo Switch
===================================================
Copy this folder to the root of the microSD as sdmc:/switch/pokenx/ (pokenx.nro inside), then open PokeNX from the
Homebrew Menu.

  game/        put YOUR copy of the game here: the GameMaker 8.0 .exe and its data/ folder
               (or use the "Importar" tile of the menu to import the game's ZIP from the SD)
  logs/        boot.log, engine.log, crash.log, last-session.txt, informe.txt (menu > Diagnostico > X)
  saves/ shader/ cache/   auxiliary data
  soundfont.sf2           optional General MIDI SoundFont for the game's MIDI music

The game is NOT included. Open the program as an application (hold R while launching any game) to have the full
memory; in applet mode "Jugar" is blocked. Exit the game with L + R + +.
If something fails, copy logs/ (and atmosphere/crash_reports/) when asking for help.
Licenses: licenses/ (Mesa) and the LICENSE of the project repository.
'@ | Set-Content -LiteralPath (Join-Path $packageRoot 'README.txt') -Encoding utf8

# 6. Enforce the single-NRO rule.
$nroFiles = @(Get-ChildItem -LiteralPath $packageRoot -Filter *.nro -File -Recurse)
if ($nroFiles.Count -ne 1) { throw "Single-NRO rule violated: expected 1 NRO in $packageRoot, found $($nroFiles.Count)" }
$sha = (Get-FileHash -LiteralPath $nroFiles[0].FullName -Algorithm SHA256).Hash
Write-Host ("SD package ready: {0}" -f $packageRoot)
Write-Host ("Single distributable: {0} ({1} bytes, SHA-256 {2})" -f $nroFiles[0].Name, $nroFiles[0].Length, $sha)
