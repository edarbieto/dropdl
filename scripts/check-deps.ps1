# Verifies that a Windows executable only imports DLLs that ship with Windows,
# so it runs on a clean machine (no Visual C++ Redistributable, no extra DLLs).
#
# Usage: scripts/check-deps.ps1 [path\to\dropdl.exe]
param([string]$Exe = "target\release\dropdl.exe")
$ErrorActionPreference = "Stop"

$bytes = [IO.File]::ReadAllBytes((Resolve-Path $Exe))
function U16($o) { [BitConverter]::ToUInt16($bytes, $o) }
function U32($o) { [BitConverter]::ToUInt32($bytes, $o) }
function CStr($o) { $e = $o; while ($bytes[$e] -ne 0) { $e++ }; [Text.Encoding]::ASCII.GetString($bytes, $o, $e - $o) }

$pe = U32 0x3C
if ((U32 $pe) -ne 0x4550) { throw "$Exe is not a PE file" }
$sections = U16 ($pe + 6)
$optSize = U16 ($pe + 20)
$opt = $pe + 24
$dataDirs = if ((U16 $opt) -eq 0x20B) { $opt + 112 } else { $opt + 96 }   # PE32+ / PE32
$sectionTable = $opt + $optSize

function Offset($rva) {
    for ($i = 0; $i -lt $sections; $i++) {
        $s = $sectionTable + 40 * $i
        $va = U32 ($s + 12); $size = [Math]::Max((U32 ($s + 8)), (U32 ($s + 16))); $raw = U32 ($s + 20)
        if ($rva -ge $va -and $rva -lt $va + $size) { return $rva - $va + $raw }
    }
    throw "RVA $rva not in any section"
}

$dlls = @()
# Regular imports: 20-byte descriptors, name RVA at +12.
$rva = U32 ($dataDirs + 8 * 1)
if ($rva) { for ($d = Offset $rva; (U32 ($d + 12)) -ne 0; $d += 20) { $dlls += CStr (Offset (U32 ($d + 12))) } }
# Delay-load imports: 32-byte descriptors, name RVA at +4.
$rva = U32 ($dataDirs + 8 * 13)
if ($rva) { for ($d = Offset $rva; (U32 ($d + 4)) -ne 0; $d += 32) { $dlls += CStr (Offset (U32 ($d + 4))) } }

$system = Join-Path $env:WINDIR "System32"
$bad = @()
foreach ($dll in ($dlls | Sort-Object -Unique)) {
    $lower = $dll.ToLower()
    $ok = ($lower -like "api-ms-win-core-*" -or $lower -like "ext-ms-*") -or
          ((Test-Path (Join-Path $system $dll)) -and $lower -notmatch '^(vcruntime|msvcp|api-ms-win-crt-)')
    "{0,-4} {1}" -f ($(if ($ok) { "ok" } else { "BAD" })), $dll
    if (-not $ok) { $bad += $dll }
}
if ($bad) { Write-Error "Non-system DLL dependencies: $($bad -join ', ')"; exit 1 }
"All $(($dlls | Sort-Object -Unique).Count) imported DLLs ship with Windows."
