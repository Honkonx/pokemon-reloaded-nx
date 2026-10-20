# Builds the HOME shortcut ("forwarder") of PokeNX as an installable NSP, ON THE PC.
# The shortcut is a tiny title whose program starts sdmc:/switch/pokenx/pokenx.nro (app/forwarder: a patched nx-hbloader).
# Installing it puts a PokeNX icon on the console's HOME menu and starts the program as an application (full memory).
#
# What you need (nothing of it is in the repository or the NRO):
#   -Keys          your console's prod.keys (dumped from your own console; it is only read, never copied or logged)
#   -Hacbrewpack   hacbrewpack.exe (https://github.com/The-4n/hacBrewPack/releases) packs the NCAs with those keys
# Installing the NSP on the console needs a title installer (DBI, Tinfoil, Goldleaf...) and signature patches
# ("sigpatches") for Atmosphere, like any homebrew forwarder.
#
# Not verified on a console: the NSP has not been installed or launched by this project yet.
#
# Usage: pwsh scripts/build-nsp.ps1 -Keys <prod.keys> -Hacbrewpack <hacbrewpack.exe> [-Output <folder>] [-DryRun]
#   -DryRun lays out the hacbrewpack folders and stops before packing (for checking the layout; needs no keys).
param(
    [string]$Keys,
    [string]$Hacbrewpack,
    [string]$Output = (Join-Path $PSScriptRoot '..\nsp-home'),
    [string]$DevkitPro = 'C:\devkitPro',
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$Output = [IO.Path]::GetFullPath($Output)
$titleId = '01007E3A9F2C4000'   # must match app/forwarder/forwarder.json

if (-not $DryRun) {
    if (-not $Keys -or -not (Test-Path -LiteralPath $Keys -PathType Leaf)) { throw 'Pass -Keys <your prod.keys>.' }
    if (-not $Hacbrewpack -or -not (Test-Path -LiteralPath $Hacbrewpack -PathType Leaf)) { throw 'Pass -Hacbrewpack <path to hacbrewpack.exe>.' }
}

# 1. The forwarder program (ExeFS: main + main.npdm), built with devkitPro.
$forwarder = Join-Path $root 'app/forwarder/pokenx-forwarder.nsp'
if (-not (Test-Path -LiteralPath $forwarder -PathType Leaf)) {
    & (Join-Path $root 'app/forwarder/build.ps1') -DevkitPro $DevkitPro
}
if (-not (Test-Path -LiteralPath $forwarder -PathType Leaf)) { throw 'The forwarder was not built.' }

# 2. Unpack its PFS0 (the ExeFS) into the folder layout hacbrewpack reads.
$work = Join-Path $Output 'work'
if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
$exefs = New-Item -ItemType Directory -Force -Path (Join-Path $work 'exefs')
$control = New-Item -ItemType Directory -Force -Path (Join-Path $work 'control')
$bytes = [IO.File]::ReadAllBytes($forwarder)
if ([Text.Encoding]::ASCII.GetString($bytes, 0, 4) -ne 'PFS0') { throw 'pokenx-forwarder.nsp is not a PFS0.' }
$count = [BitConverter]::ToUInt32($bytes, 4)
$stringTableSize = [BitConverter]::ToUInt32($bytes, 8)
$entriesAt = 16
$namesAt = $entriesAt + 24 * $count
$dataAt = $namesAt + $stringTableSize
for ($i = 0; $i -lt $count; $i++) {
    $offset = [BitConverter]::ToUInt64($bytes, $entriesAt + 24 * $i)
    $size = [BitConverter]::ToUInt64($bytes, $entriesAt + 24 * $i + 8)
    $nameOffset = [BitConverter]::ToUInt32($bytes, $entriesAt + 24 * $i + 16)
    $end = $namesAt + $nameOffset
    while ($bytes[$end] -ne 0) { $end++ }
    $name = [Text.Encoding]::ASCII.GetString($bytes, $namesAt + $nameOffset, $end - ($namesAt + $nameOffset))
    $slice = New-Object byte[] $size
    [Array]::Copy($bytes, $dataAt + $offset, $slice, 0, $size)
    [IO.File]::WriteAllBytes((Join-Path $exefs.FullName $name), $slice)
}
foreach ($required in 'main', 'main.npdm') {
    if (-not (Test-Path -LiteralPath (Join-Path $exefs.FullName $required))) { throw "The ExeFS has no $required." }
}

# 3. Control data: the title's name and icon (HOME shows them).
$nacptool = Join-Path $DevkitPro 'tools\bin\nacptool.exe'
if (-not (Test-Path -LiteralPath $nacptool)) { throw "nacptool.exe was not found in $DevkitPro." }
& $nacptool --create 'PokeNX' 'PokeNX Project' '1.0.0' (Join-Path $control.FullName 'control.nacp') "--titleid=$titleId"
if ($LASTEXITCODE -ne 0) { throw 'nacptool failed.' }
Copy-Item -LiteralPath (Join-Path $root 'app/assets/pokenx-icon-256.jpg') -Destination (Join-Path $control.FullName 'icon_AmericanEnglish.dat') -Force

Write-Host "Layout ready in $work"
foreach ($file in Get-ChildItem -LiteralPath $work -Recurse -File) { Write-Host ('  {0}  ({1} bytes)' -f $file.FullName.Substring($work.Length + 1), $file.Length) }
if ($DryRun) { Write-Host 'DryRun: stopped before packing.'; return }

# 4. Pack NCAs and the NSP with hacbrewpack (it needs a folder of its own to work in).
Push-Location $work
try {
    & $Hacbrewpack --keyset $Keys --titleid $titleId --nopatchnacplogo
    if ($LASTEXITCODE -ne 0) { throw "hacbrewpack failed with exit code $LASTEXITCODE." }
} finally { Pop-Location }
$nsp = Get-ChildItem -LiteralPath (Join-Path $work 'hacbrewpack_nsp') -Filter '*.nsp' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $nsp) { throw 'hacbrewpack produced no NSP.' }
$result = Join-Path $Output 'PokeNX-HOME.nsp'
Copy-Item -LiteralPath $nsp.FullName -Destination $result -Force
$hash = (Get-FileHash -LiteralPath $result -Algorithm SHA256).Hash
Write-Host ("NSP: {0} ({1} bytes, SHA-256 {2})" -f $result, (Get-Item $result).Length, $hash)
Write-Host 'Install it with a title installer (DBI, Tinfoil, Goldleaf). It needs sigpatches. Not verified on a console.'
