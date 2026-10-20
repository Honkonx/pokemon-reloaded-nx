# Builds the HOME-shortcut forwarder (a patched nx-hbloader that starts sdmc:/switch/pokenx/pokenx.nro): produces
# pokenx-forwarder.nsp, an ExeFS (main + main.npdm) that scripts/build-nsp.ps1 packs into an installable NSP.
# Needs devkitPro (the Switch group) in $DevkitPro.
param([string]$DevkitPro = 'C:\devkitPro')
$ErrorActionPreference = 'Stop'
$bash = Join-Path $DevkitPro 'msys2\usr\bin\bash.exe'
if (-not (Test-Path -LiteralPath $bash)) { throw "devkitPro's MSYS2 was not found in $DevkitPro (use -DevkitPro <folder>)" }
$here = $PSScriptRoot.Replace('\', '/')
$drive = $here.Substring(0, 1).ToLower()
$posix = "/$drive" + $here.Substring(2)
& $bash -lc "export DEVKITPRO=/opt/devkitpro; cd '$posix' && make clean >/dev/null 2>&1; make 2>&1"
if ($LASTEXITCODE -ne 0) { throw 'the forwarder did not build' }
$nsp = Join-Path $PSScriptRoot 'pokenx-forwarder.nsp'
if (-not (Test-Path -LiteralPath $nsp)) { throw 'pokenx-forwarder.nsp was not produced' }
Get-Item $nsp | Select-Object FullName, Length
