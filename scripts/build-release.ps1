# Builds the portable release: dist/dropdl.exe and dist/THIRD-PARTY-LICENSES.txt
#
# Usage: scripts/build-release.ps1 [-Upx]
#   -Upx  also compress with UPX (upx must be on PATH or set via $env:UPX)
param([switch]$Upx)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    & "$PSScriptRoot\check-deps.ps1" "target\release\dropdl.exe"
    if ($LASTEXITCODE -ne 0) { throw "dependency check failed" }

    New-Item -ItemType Directory -Force dist | Out-Null
    Copy-Item target\release\dropdl.exe dist\dropdl.exe -Force
    Copy-Item THIRD-PARTY-LICENSES.txt dist\THIRD-PARTY-LICENSES.txt -Force

    if ($Upx) {
        $upxExe = if ($env:UPX) { $env:UPX } else { (Get-Command upx -ErrorAction Stop).Source }
        & $upxExe --best --lzma -q dist\dropdl.exe | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "upx failed" }
    }

    $size = (Get-Item dist\dropdl.exe).Length
    "dist\dropdl.exe  {0:N2} MB{1}" -f ($size / 1MB), $(if ($Upx) { " (UPX)" } else { "" })
}
finally {
    Pop-Location
}
