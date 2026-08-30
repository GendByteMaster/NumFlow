[CmdletBinding()]
param(
    [string]$PackagePath,

    [switch]$ForceNewCertificate,

    [switch]$PassThru
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common-driver-tools.ps1')

$subject = 'CN=NumFlow Driver Test'
$codeSigningEku = '1.3.6.1.5.5.7.3.3'
$resolvedPackagePath = Resolve-NumFlowPackagePath -PackagePath $PackagePath
$packageFiles = Get-NumFlowPackageFiles -PackagePath $resolvedPackagePath
$tools = Get-NumFlowDriverTools

function Test-CodeSigningCertificate {
    param(
        [Parameter(Mandatory)]
        [System.Security.Cryptography.X509Certificates.X509Certificate2]$Certificate
    )

    $hasCodeSigningEku = $Certificate.Extensions |
        Where-Object { $_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] } |
        ForEach-Object { $_.EnhancedKeyUsages } |
        Where-Object { $_.Value -eq $codeSigningEku }

    return $Certificate.Subject -eq $subject -and
        $Certificate.HasPrivateKey -and
        $Certificate.NotBefore -le (Get-Date) -and
        $Certificate.NotAfter -gt (Get-Date).AddDays(30) -and
        $null -ne $hasCodeSigningEku
}

function New-NumFlowNonExportableCertificate {
    # Invoke the inbox PKI cmdlet in Windows PowerShell directly. Importing the PKI module through
    # PowerShell 7 compatibility creates a broker process that is unreliable in noninteractive CI.
    $windowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
    $creationScript = @'
$ErrorActionPreference = 'Stop'
$certificate = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject 'CN=NumFlow Driver Test' `
    -CertStoreLocation 'Cert:\CurrentUser\My' `
    -KeyAlgorithm RSA `
    -KeyLength 3072 `
    -HashAlgorithm SHA256 `
    -KeyExportPolicy NonExportable `
    -NotAfter (Get-Date).AddYears(2) `
    -FriendlyName 'NumFlow Driver Test (VM only)'
$certificate.Thumbprint
'@
    $creationOutput = @(& $windowsPowerShell -NoProfile -NonInteractive -Command $creationScript 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "Windows PowerShell certificate creation failed: $($creationOutput -join ' ')"
    }
    $thumbprint = ($creationOutput | Select-Object -Last 1).ToString().Trim()
    if ($thumbprint -notmatch '^[0-9A-Fa-f]{40}$') {
        throw "Certificate creation returned an invalid thumbprint: $thumbprint"
    }
    $storedCertificate = Get-ChildItem -LiteralPath Cert:\CurrentUser\My |
        Where-Object Thumbprint -eq $thumbprint |
        Select-Object -First 1
    if (-not $storedCertificate -or -not $storedCertificate.HasPrivateKey) {
        throw 'The new NumFlow test certificate was not persisted with its private key.'
    }
    return $storedCertificate
}

$certificateState = 'FOUND'
$certificate = $null
if (-not $ForceNewCertificate) {
    $certificate = Get-ChildItem -LiteralPath Cert:\CurrentUser\My |
        Where-Object { Test-CodeSigningCertificate -Certificate $_ } |
        Sort-Object NotAfter, NotBefore -Descending |
        Select-Object -First 1
}

if (-not $certificate) {
    $certificateState = 'CREATED'
    $certificate = New-NumFlowNonExportableCertificate
}

if (-not (Test-CodeSigningCertificate -Certificate $certificate)) {
    throw 'The selected certificate does not meet NumFlow test-signing requirements.'
}

$rsa = [System.Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey(
    $certificate
)
try {
    if ($rsa -isnot [System.Security.Cryptography.RSACng]) {
        throw 'The selected private key is not a CNG key; its non-exportable policy cannot be proven.'
    }
    $exportPolicy = $rsa.Key.ExportPolicy
    if (($exportPolicy -band [System.Security.Cryptography.CngExportPolicies]::AllowExport) -ne 0 -or
        ($exportPolicy -band [System.Security.Cryptography.CngExportPolicies]::AllowPlaintextExport) -ne 0) {
        throw 'The selected private key is exportable; refusing to use it.'
    }
}
finally {
    if ($rsa) {
        $rsa.Dispose()
    }
}

$preSignSysHash = (Get-FileHash -LiteralPath $packageFiles.Sys -Algorithm SHA256).Hash
$preSignInfHash = (Get-FileHash -LiteralPath $packageFiles.Inf -Algorithm SHA256).Hash
[System.IO.File]::WriteAllBytes(
    $packageFiles.Cer,
    $certificate.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Cert)
)

[void](Invoke-NumFlowTool -FilePath $tools.SignTool -Arguments @(
        'sign',
        '/v',
        '/fd', 'SHA256',
        '/sha1', $certificate.Thumbprint,
        '/s', 'My',
        $packageFiles.Cat
    ))

if ((Get-FileHash -LiteralPath $packageFiles.Sys -Algorithm SHA256).Hash -ne $preSignSysHash -or
    (Get-FileHash -LiteralPath $packageFiles.Inf -Algorithm SHA256).Hash -ne $preSignInfHash) {
    throw 'Signing unexpectedly changed the staged SYS or INF.'
}

[void](Test-NumFlowCatalogSignatureOffline -CatalogPath $packageFiles.Cat -CertificatePath $packageFiles.Cer)

$hostTrustOutput = @(& $tools.SignTool verify /v /pa /all $packageFiles.Cat 2>&1)
$hostTrustExitCode = $LASTEXITCODE
$hostTrustOutput | ForEach-Object { Write-Host $_ }
$hostTrustText = $hostTrustOutput -join [Environment]::NewLine
$untrustedRootPattern = '(?is)certificate\s+which is not trusted by the trust provider|CERT_E_UNTRUSTEDROOT|0x800B0109'
$hostTrustStatus = if ($hostTrustExitCode -eq 0) {
    'PASS'
}
elseif ($hostTrustText -match $untrustedRootPattern) {
    'UNTRUSTED_ROOT_EXPECTED'
}
else {
    throw "SignTool host verification failed for a reason other than the expected untrusted root."
}

foreach ($memberPath in @($packageFiles.Sys, $packageFiles.Inf)) {
    $memberOutput = @(& $tools.SignTool verify /v /pa /c $packageFiles.Cat $memberPath 2>&1)
    $memberExitCode = $LASTEXITCODE
    $memberOutput | ForEach-Object { Write-Host $_ }
    $memberText = $memberOutput -join [Environment]::NewLine
    if ($memberText -notmatch '(?im)^File is signed in catalog:') {
        throw "Catalog membership was not confirmed for $memberPath."
    }
    if ($memberExitCode -ne 0 -and $memberText -notmatch $untrustedRootPattern) {
        throw "Catalog membership verification failed for $memberPath."
    }
}

$result = [pscustomobject]@{
    PackagePath             = $resolvedPackagePath
    Certificate             = $certificateState
    CertificateSubject      = $certificate.Subject
    CertificateThumbprint   = $certificate.Thumbprint
    CertificateNotAfter     = $certificate.NotAfter
    PrivateKeyExportable    = $false
    CatalogSigning          = 'PASS'
    OfflineSignature        = 'PASS'
    HostSignToolTrust       = $hostTrustStatus
    HostSignToolExitCode    = $hostTrustExitCode
}

if ($PassThru) {
    return $result
}
$result | Format-List
