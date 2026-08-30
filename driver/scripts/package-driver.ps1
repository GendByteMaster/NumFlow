[CmdletBinding()]
param(
    [string]$PackagePath,

    [string[]]$OperatingSystems = @('10_CO_X64', '10_NI_X64', '10_GE_X64'),

    [switch]$PassThru
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common-driver-tools.ps1')

$repositoryRoot = Get-NumFlowRepositoryRoot
$resolvedPackagePath = Resolve-NumFlowPackagePath -PackagePath $PackagePath
$buildPath = Join-Path $repositoryRoot 'driver\x64\Release'
$sourceSys = Join-Path $buildPath 'numflow-kbd-filter.sys'
$sourceInf = Join-Path $buildPath 'numflow-kbd-filter.inf'

foreach ($sourcePath in @($sourceSys, $sourceInf)) {
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "Release build output is missing: $sourcePath"
    }
}

$infText = Get-Content -LiteralPath $sourceInf -Raw
if ($infText -notmatch '(?im)^CatalogFile\s*=\s*numflow-kbd-filter\.cat\s*$') {
    throw 'Release INF does not declare CatalogFile=numflow-kbd-filter.cat.'
}

$resolvedPackagePath = Clear-NumFlowPackageDirectory -PackagePath $resolvedPackagePath
Copy-Item -LiteralPath $sourceSys -Destination $resolvedPackagePath
Copy-Item -LiteralPath $sourceInf -Destination $resolvedPackagePath

$tools = Get-NumFlowDriverTools
$stagedInf = Join-Path $resolvedPackagePath 'numflow-kbd-filter.inf'
[void](Invoke-NumFlowTool -FilePath $tools.InfVerif -Arguments @('/v', $stagedInf))
[void](Invoke-NumFlowTool -FilePath $tools.Inf2Cat -Arguments @(
        "/driver:$resolvedPackagePath",
        "/os:$($OperatingSystems -join ',')",
        '/verbose'
    ))

$packageFiles = Get-NumFlowPackageFiles -PackagePath $resolvedPackagePath
$result = [pscustomobject]@{
    PackagePath      = $resolvedPackagePath
    WdkVersion       = $tools.Version
    OperatingSystems = $OperatingSystems
    InfVerif         = 'PASS'
    Inf2Cat          = 'PASS'
    CatalogPath      = $packageFiles.Cat
}

if ($PassThru) {
    return $result
}
$result | Format-List
