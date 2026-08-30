[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$PackagePath,

    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common-vm.ps1')

Assert-NumFlowVirtualBoxVm
$files = Get-NumFlowVmPackageFiles -PackagePath $PackagePath
Test-NumFlowVmChecksums -Files $files
$thumbprint = Test-NumFlowVmCatalogSignature -Files $files

$enumOutput = @(& pnputil.exe /enum-drivers 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw 'pnputil /enum-drivers failed.'
}
$blocks = ($enumOutput -join "`n") -split '(?:\r?\n){2,}'
$publishedNames = @(
    @(
        foreach ($block in $blocks) {
            if ($block -match '(?im)numflow-kbd-filter\.inf') {
                [regex]::Matches($block, '(?im)\boem\d+\.inf\b') |
                    ForEach-Object { $_.Value.ToLowerInvariant() }
            }
        }
    ) | Sort-Object -Unique
)

if ($publishedNames.Count -ne 1) {
    throw "Expected exactly one staged NumFlow package; found $($publishedNames.Count): $($publishedNames -join ', ')"
}
$publishedName = $publishedNames[0]
Write-Host "Exact VM package selected for removal: $publishedName (numflow-kbd-filter.inf)"

if (-not $Force) {
    $confirmation = Read-Host "Type $publishedName to uninstall only this NumFlow package"
    if ($confirmation -ne $publishedName) {
        throw 'Removal cancelled: confirmation did not match the published INF name.'
    }
}

[void](Invoke-NumFlowVmNativeTool -FilePath 'pnputil.exe' -Arguments @(
        '/delete-driver', $publishedName, '/uninstall'
    ) -AllowedExitCodes @(0, 3010))

foreach ($storeName in @('Root', 'TrustedPublisher')) {
    $certificatePath = "Cert:\LocalMachine\$storeName\$thumbprint"
    if (Test-Path -LiteralPath $certificatePath) {
        Remove-Item -LiteralPath $certificatePath -Force
        Write-Host "Removed exact VM certificate from LocalMachine\${storeName}: $thumbprint"
    }
}

Write-Host 'NumFlow package removal completed. Do not remove kbdclass, keyboard.inf, or unrelated filters.'
Write-Host 'Reboot the VM manually, validate keyboard input, and restore the known-good snapshot if recovery is incomplete.'
