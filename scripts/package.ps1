param(
    [switch]$SkipBuild,
    [ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9._-]*$')]
    [string]$Version = "0.1.0"
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$binary = Join-Path $projectRoot 'target/release/mimonitor_toolbox_ddc.exe'

if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'build.ps1') -Release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
if (-not (Test-Path -LiteralPath $binary -PathType Leaf) -or (Get-Item -LiteralPath $binary).Length -eq 0) {
    throw "Release EXE not found: $binary"
}

# Refuse an executable that still needs a redistributable or companion DLL.
$dumpbin = (Get-Command dumpbin.exe -ErrorAction SilentlyContinue).Source
if (-not $dumpbin) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (Test-Path -LiteralPath $vswhere) {
        $dumpbin = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -find 'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe' |
            Sort-Object -Descending | Select-Object -First 1
    }
}
if (-not $dumpbin) { throw 'dumpbin.exe is required to verify single-EXE dependencies' }
$dependencies = & $dumpbin /DEPENDENTS $binary
if ($LASTEXITCODE -ne 0) { throw 'Could not inspect Release EXE dependencies' }
$dlls = @($dependencies | ForEach-Object {
    if ($_ -match '^\s+([a-zA-Z0-9_.-]+\.dll)\s*$') { $Matches[1] }
})
if ($dlls.Count -eq 0) { throw 'No PE dependencies found; refusing an unchecked EXE' }
foreach ($dll in $dlls) {
    if ($dll -match '^(VCRUNTIME|MSVCP|MSVCR|CONCRT)' -or
        ($dll -notmatch '^(api-ms-win-|ext-ms-win-)' -and -not (Test-Path -LiteralPath (Join-Path $env:SystemRoot "System32/$dll")))) {
        throw "Single-EXE release still requires an external DLL: $dll"
    }
}

$outputDirectory = Join-Path $projectRoot "dist/single-exe-v$Version"
New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
if (@(Get-ChildItem -LiteralPath $outputDirectory -Force | Where-Object Name -ne 'MimonitorToolbox.exe').Count -ne 0) {
    throw "Output directory contains other files: $outputDirectory"
}
$output = Join-Path $outputDirectory 'MimonitorToolbox.exe'
Copy-Item -LiteralPath $binary -Destination $output -Force
$hash = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
Write-Host "Single-EXE release: $output"
Write-Host "SHA256: $hash"
# The hash is printed; no ZIP, checksum sidecar, asset directory or second edition is emitted.
