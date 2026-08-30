[CmdletBinding()]
param(
    [switch]$ForceNewCertificate
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common-driver-tools.ps1')

$repositoryRoot = Get-NumFlowRepositoryRoot
$packagePath = Resolve-NumFlowPackagePath

Write-Host '== NumFlow PASS_THROUGH Release build =='
& (Join-Path $repositoryRoot 'driver\build.ps1') -Configuration Release -Rebuild

Write-Host '== NumFlow test package and catalog =='
$packageResult = & (Join-Path $PSScriptRoot 'package-driver.ps1') -PackagePath $packagePath -PassThru

Write-Host '== NumFlow non-exportable test certificate and CAT signing =='
$signArguments = @{
    PackagePath = $packagePath
    PassThru    = $true
}
if ($ForceNewCertificate) {
    $signArguments.ForceNewCertificate = $true
}
$signResult = & (Join-Path $PSScriptRoot 'sign-test-driver.ps1') @signArguments

$manifestPath = Write-NumFlowChecksumManifest -PackagePath $packagePath
[void](Test-NumFlowChecksumManifest -PackagePath $packagePath)

$expectedNames = @(
    'numflow-kbd-filter.sys',
    'numflow-kbd-filter.inf',
    'numflow-kbd-filter.cat',
    'NumFlowDriverTest.cer',
    'SHA256SUMS.txt'
)
$actualNames = @(Get-ChildItem -LiteralPath $packagePath -File | Select-Object -ExpandProperty Name)
if (@($actualNames | Where-Object { $_ -notin $expectedNames }).Count -ne 0 -or
    @($expectedNames | Where-Object { $_ -notin $actualNames }).Count -ne 0) {
    throw "Package contains an unexpected file set: $($actualNames -join ', ')"
}

Write-Host ''
Write-Host 'NumFlow TestPackage summary'
Write-Host '  Build:                 PASS'
Write-Host "  InfVerif:              $($packageResult.InfVerif)"
Write-Host "  Inf2Cat:               $($packageResult.Inf2Cat)"
Write-Host "  Certificate:           $($signResult.Certificate)"
Write-Host "  Certificate thumbprint:$($signResult.CertificateThumbprint)"
Write-Host "  CAT signing:           $($signResult.CatalogSigning)"
Write-Host "  Offline signature:     $($signResult.OfflineSignature)"
Write-Host "  Host SignTool trust:   $($signResult.HostSignToolTrust)"
Write-Host '  SHA256SUMS:             PASS'
Write-Host "  Manifest:               $manifestPath"
Write-Host "  Package:                $packagePath"
Write-Host ''
Write-Host 'Host safety boundary preserved: no driver install/load, trust-store import, test-signing change, or reboot was performed.'
