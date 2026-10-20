# Packs the SD folder produced by app/build-package.ps1 into a release ZIP:
#   switch/pokenx/...   <- extract the ZIP at the root of the microSD
#   LICENSE, NOTICE-*.md, LEEME.txt, licenses/
# The ZIP never contains the game. Output: release/pokemon-reloaded-nx-<version>-switch.zip (+ .sha256).
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$package = Join-Path $root 'pokenx'
if (-not (Test-Path -LiteralPath (Join-Path $package 'pokenx.nro'))) { throw 'Run app/build-package.ps1 first.' }
$game = Get-ChildItem -LiteralPath (Join-Path $package 'game') -File -Force -Recurse | Where-Object { $_.Name -ne '.gitkeep' }
if ($game) { throw 'pokenx/game contains files; refusing to build a release.' }

$stage = Join-Path ([IO.Path]::GetTempPath()) ("pokenx-release-" + [guid]::NewGuid())
$target = Join-Path $stage 'switch/pokenx'
New-Item -ItemType Directory -Force -Path $target | Out-Null
foreach ($directory in 'game', 'game/data', 'logs', 'shader', 'cache', 'saves') {
    New-Item -ItemType Directory -Force -Path (Join-Path $target $directory) | Out-Null
    # An empty folder is not stored by every ZIP tool: keep a marker so the layout survives.
    Set-Content -LiteralPath (Join-Path $target "$directory/.gitkeep") -Value '' -NoNewline
}
foreach ($file in 'pokenx.nro', 'README.txt', 'graphics-build.json', 'graphics-driver.txt', 'cover.png', 'icon.png', 'soundfont.sf2', 'SOUNDFONT-LICENSE.txt') {
    $path = Join-Path $package $file
    if (Test-Path -LiteralPath $path) { Copy-Item -LiteralPath $path -Destination $target }
}
if (Test-Path -LiteralPath (Join-Path $package 'licenses')) { Copy-Item -LiteralPath (Join-Path $package 'licenses') -Destination (Join-Path $stage 'licenses') -Recurse }
foreach ($file in 'LICENSE', 'NOTICE-android-mit.md', 'NOTICE-mesa26.md', 'THIRD_PARTY.md') {
    $path = Join-Path $root $file
    if (Test-Path -LiteralPath $path) { Copy-Item -LiteralPath $path -Destination $stage }
}
$nro = Join-Path $target 'pokenx.nro'
$sha = (Get-FileHash -LiteralPath $nro -Algorithm SHA256).Hash
@"
PokeNX / pokemon-reloaded-nx $Version
=====================================
Native OpenGMK runtime for Nintendo Switch. This ZIP does NOT contain the game.

1. Extract this ZIP at the root of your microSD (it creates switch/pokenx/).
2. Put your own copy of the game (the .exe and its data/ folder) in switch/pokenx/game/,
   or use the "Importar" tile of the menu to import the game's ZIP from the SD.
3. Open "PokeNX" from the Homebrew Menu.

NRO SHA-256: $sha
Source code for this exact build: the Git tag $Version of the project repository.
Licenses: LICENSE (GPL-2.0), NOTICE-*.md and licenses/ (Mesa).
"@ | Set-Content -LiteralPath (Join-Path $stage 'LEEME.txt') -Encoding utf8

New-Item -ItemType Directory -Force -Path (Join-Path $root 'release') | Out-Null
$zip = Join-Path $root "release/pokemon-reloaded-nx-$Version-switch.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Remove-Item -LiteralPath $stage -Recurse -Force
$zipSha = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash
"$zipSha  $(Split-Path $zip -Leaf)" | Set-Content -LiteralPath "$zip.sha256" -Encoding ascii
Write-Host "ZIP: $zip ($((Get-Item $zip).Length) bytes) SHA-256 $zipSha"
Write-Host "NRO SHA-256: $sha"
