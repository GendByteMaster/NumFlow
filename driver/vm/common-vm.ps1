Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Assert-NumFlowVirtualBoxVm {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run this script from an elevated PowerShell session inside the test VM.'
    }

    $computerSystem = Get-CimInstance -ClassName Win32_ComputerSystem
    $vmIdentity = "$($computerSystem.Manufacturer) $($computerSystem.Model)"
    if ($vmIdentity -notmatch '(?i)virtualbox|oracle|innotek') {
        throw "Safety stop: this machine is not identified as VirtualBox ($vmIdentity)."
    }
    Write-Host "VirtualBox guard: PASS ($vmIdentity)"
}

function Get-NumFlowVmPackageFiles {
    param(
        [Parameter(Mandatory)]
        [string]$PackagePath
    )

    $resolvedPackagePath = (Resolve-Path -LiteralPath $PackagePath).Path
    $files = [ordered]@{
        PackagePath = $resolvedPackagePath
        Sys = Join-Path $resolvedPackagePath 'numflow-kbd-filter.sys'
        Inf = Join-Path $resolvedPackagePath 'numflow-kbd-filter.inf'
        Cat = Join-Path $resolvedPackagePath 'numflow-kbd-filter.cat'
        Cer = Join-Path $resolvedPackagePath 'NumFlowDriverTest.cer'
        Manifest = Join-Path $resolvedPackagePath 'SHA256SUMS.txt'
    }
    foreach ($key in @('Sys', 'Inf', 'Cat', 'Cer', 'Manifest')) {
        if (-not (Test-Path -LiteralPath $files[$key] -PathType Leaf)) {
            throw "Required package file is missing: $($files[$key])"
        }
    }
    return [pscustomobject]$files
}

function Test-NumFlowVmChecksums {
    param(
        [Parameter(Mandatory)]
        [pscustomobject]$Files
    )

    $expectedNames = @(
        'numflow-kbd-filter.sys',
        'numflow-kbd-filter.inf',
        'numflow-kbd-filter.cat',
        'NumFlowDriverTest.cer'
    )
    $seenNames = @()
    foreach ($line in Get-Content -LiteralPath $Files.Manifest) {
        if ($line -notmatch '^([0-9A-Fa-f]{64}) \*([^\\/]+)$') {
            throw "Invalid SHA256SUMS.txt line: $line"
        }
        $expectedHash = $Matches[1].ToUpperInvariant()
        $name = $Matches[2]
        if ($name -notin $expectedNames -or $name -in $seenNames) {
            throw "Unexpected or duplicate checksum entry: $name"
        }
        $seenNames += $name
        $actualHash = (Get-FileHash -LiteralPath (Join-Path $Files.PackagePath $name) -Algorithm SHA256).Hash
        if ($actualHash -ne $expectedHash) {
            throw "SHA-256 mismatch for $name"
        }
    }
    if ($seenNames.Count -ne $expectedNames.Count) {
        throw 'SHA256SUMS.txt does not contain every required package file.'
    }
    Write-Host 'SHA256SUMS: PASS'
}

function Test-NumFlowVmCatalogSignature {
    param(
        [Parameter(Mandatory)]
        [pscustomobject]$Files
    )

    Add-Type -AssemblyName System.Security
    $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($Files.Cer)
    try {
        if ($certificate.Subject -ne 'CN=NumFlow Driver Test') {
            throw "Unexpected package certificate subject: $($certificate.Subject)"
        }
        if ($certificate.Issuer -ne $certificate.Subject -or
            $certificate.NotBefore -gt (Get-Date) -or
            $certificate.NotAfter -le (Get-Date)) {
            throw 'The package certificate is not a currently valid self-signed NumFlow test certificate.'
        }
        $codeSigningEku = $certificate.Extensions |
            Where-Object { $_ -is [Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] } |
            ForEach-Object { $_.EnhancedKeyUsages } |
            Where-Object { $_.Value -eq '1.3.6.1.5.5.7.3.3' }
        if ($null -eq $codeSigningEku -or $certificate.PublicKey.Key.KeySize -lt 3072) {
            throw 'The package certificate does not meet the code-signing EKU and RSA key-size policy.'
        }
        $signedCms = [Security.Cryptography.Pkcs.SignedCms]::new()
        $signedCms.Decode([IO.File]::ReadAllBytes($Files.Cat))
        $signedCms.CheckSignature($true)
        if ($signedCms.SignerInfos.Count -ne 1 -or
            $signedCms.SignerInfos[0].Certificate.Thumbprint -ne $certificate.Thumbprint) {
            throw 'CAT signer does not match NumFlowDriverTest.cer.'
        }
        Write-Host "CAT signature: PASS ($($certificate.Thumbprint))"
        return $certificate.Thumbprint
    }
    finally {
        $certificate.Dispose()
    }
}

function Invoke-NumFlowVmNativeTool {
    param(
        [Parameter(Mandatory)]
        [string]$FilePath,

        [Parameter(Mandatory)]
        [string[]]$Arguments,

        [int[]]$AllowedExitCodes = @(0)
    )

    $output = @(& $FilePath @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    $output | ForEach-Object { Write-Host $_ }
    if ($exitCode -notin $AllowedExitCodes) {
        throw "$FilePath failed with exit code $exitCode."
    }
    return [pscustomobject]@{
        ExitCode = $exitCode
        Output = ($output -join [Environment]::NewLine)
    }
}
