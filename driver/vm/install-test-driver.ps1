[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$PackagePath,

    [switch]$Reboot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common-vm.ps1')

Assert-NumFlowVirtualBoxVm
$files = Get-NumFlowVmPackageFiles -PackagePath $PackagePath
Test-NumFlowVmChecksums -Files $files
$thumbprint = Test-NumFlowVmCatalogSignature -Files $files

$bootConfiguration = @(& bcdedit.exe /enum '{current}' 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw 'Unable to read the current VM boot configuration with bcdedit.'
}
if (($bootConfiguration -join [Environment]::NewLine) -notmatch '(?im)^testsigning\s+Yes\s*$') {
    throw 'TESTSIGNING is not enabled in this VM. Enable it, reboot the VM, and rerun this script.'
}
Write-Host 'VM TESTSIGNING: PASS'

if (Get-Command Confirm-SecureBootUEFI -ErrorAction SilentlyContinue) {
    try {
        if (Confirm-SecureBootUEFI) {
            throw 'Secure Boot is enabled. Disable it in the disposable VM before test-driver installation.'
        }
    }
    catch [System.PlatformNotSupportedException] {
        Write-Host 'Secure Boot status is not supported by this VM firmware; continuing.'
    }
}

$storeNames = @('Root', 'TrustedPublisher')
$preexistingTrust = @{}
foreach ($storeName in $storeNames) {
    $preexistingTrust[$storeName] = Test-Path -LiteralPath
        "Cert:\LocalMachine\$storeName\$thumbprint"
}

try {
    Import-Certificate -FilePath $files.Cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
    Import-Certificate -FilePath $files.Cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher | Out-Null
    Write-Host "Imported public test certificate into VM trust stores: $thumbprint"

    $authenticode = Get-AuthenticodeSignature -LiteralPath $files.Cat
    if ($authenticode.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
        $authenticode.SignerCertificate.Thumbprint -ne $thumbprint) {
        throw "VM trust verification failed: $($authenticode.Status) $($authenticode.StatusMessage)"
    }
    Write-Host 'VM trusted CAT verification: PASS'

    $installResult = Invoke-NumFlowVmNativeTool -FilePath 'pnputil.exe'
        -Arguments @('/add-driver', $files.Inf, '/install') -AllowedExitCodes @(0, 3010)
}
catch {
    foreach ($storeName in $storeNames) {
        $certificatePath = "Cert:\LocalMachine\$storeName\$thumbprint"
        if (-not $preexistingTrust[$storeName] -and (Test-Path -LiteralPath $certificatePath)) {
            Remove-Item -LiteralPath $certificatePath -Force
        }
    }
    throw
}

Write-Host 'NumFlow PASS_THROUGH test package installation completed in the VirtualBox VM.'
if ($installResult.ExitCode -eq 3010 -or $Reboot) {
    if ($Reboot) {
        Write-Host 'Reboot requested. The VM will restart now.'
        Restart-Computer
    }
    else {
        Write-Host 'A reboot is required. Reboot the VM manually after preserving the console and snapshot recovery path.'
    }
}
else {
    Write-Host 'Reboot only if requested by Windows or by the validation procedure.'
}
