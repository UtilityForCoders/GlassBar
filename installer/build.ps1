# Compila GlassBar en modo release y genera el instalador con Inno Setup.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = Resolve-Path "$PSScriptRoot\.."
Set-Location $Root

# Versión desde Cargo.toml
$CargoToml = Get-Content "Cargo.toml" -Raw
if ($CargoToml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "No se pudo leer la versión de Cargo.toml"
}
$Version = $Matches[1]
Write-Host "Compilando GlassBar v$Version (release)..." -ForegroundColor Cyan

cargo build --release

$Inno = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
if (-not (Test-Path $Inno)) {
    $Inno = "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
}

if (Test-Path $Inno) {
    Write-Host "Generando instalador con Inno Setup..." -ForegroundColor Cyan
    & $Inno "/DAppVersion=$Version" "installer\glassbar.iss"
    Write-Host "Instalador generado en target\installer\glassbar-setup-$Version.exe" -ForegroundColor Green
} else {
    Write-Host "Inno Setup no encontrado en el sistema. Binario release listo en target\release\glassbar.exe" -ForegroundColor Yellow
}
