Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-NumFlowRepositoryRoot {
    return [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
}

function Get-NumFlowDriverTools {
    $kitsRoot = (Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots' -Name KitsRoot10).KitsRoot10

    if (-not (Test-Path -LiteralPath $kitsRoot -PathType Container)) {
        throw "Windows Kits root was not found: $kitsRoot"
    }

    $versionDirectories = Get-ChildItem -LiteralPath (Join-Path $kitsRoot 'bin') -Directory |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending

    foreach ($versionDirectory in $versionDirectories) {
        $version = $versionDirectory.Name
        $signTool = Join-Path $versionDirectory.FullName 'x64\signtool.exe'
        $inf2CatCandidates = @(
            (Join-Path $versionDirectory.FullName 'x64\Inf2Cat.exe'),
            (Join-Path $versionDirectory.FullName 'x86\Inf2Cat.exe')
        )
        $infVerifCandidates = @(
            (Join-Path $kitsRoot "Tools\$version\x64\InfVerif.exe"),
            (Join-Path $kitsRoot "Tools\$version\x86\InfVerif.exe")
        )
        $inf2Cat = $inf2CatCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
            Select-Object -First 1
        $infVerif = $infVerifCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } |
            Select-Object -First 1

        if ((Test-Path -LiteralPath $signTool -PathType Leaf) -and $inf2Cat -and $infVerif) {
            return [pscustomobject]@{
                Version  = $version
                SignTool = $signTool
                Inf2Cat  = $inf2Cat
                InfVerif = $infVerif
            }
        }
    }

    throw 'A single installed WDK version containing SignTool, Inf2Cat, and InfVerif was not found.'
}

function Invoke-NumFlowTool {
    param(
        [Parameter(Mandatory)]
        [string]$FilePath,

        [Parameter(Mandatory)]
        [string[]]$Arguments,

        [int[]]$AllowedExitCodes = @(0),

        [switch]$Quiet
    )

    $output = @(& $FilePath @Arguments 2>&1)
    $exitCode = $LASTEXITCODE
    if (-not $Quiet) {
        $output | ForEach-Object { Write-Host $_ }
    }

    if ($exitCode -notin $AllowedExitCodes) {
        $renderedArguments = $Arguments -join ' '
        throw "Tool failed with exit code ${exitCode}: $FilePath $renderedArguments"
    }

    return [pscustomobject]@{
        ExitCode = $exitCode
        Output   = ($output -join [Environment]::NewLine)
    }
}

function Resolve-NumFlowPackagePath {
    param(
        [string]$PackagePath
    )

    $repositoryRoot = Get-NumFlowRepositoryRoot
    $packageRoot = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot 'driver\package'))
    $resolvedPackagePath = if ($PackagePath) {
        [System.IO.Path]::GetFullPath($PackagePath)
    }
    else {
        [System.IO.Path]::GetFullPath((Join-Path $packageRoot 'x64\Release'))
    }

    $requiredPrefix = $packageRoot.TrimEnd('\') + '\'
    if (-not $resolvedPackagePath.StartsWith(
            $requiredPrefix,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Package path must be a child of $packageRoot. Received: $resolvedPackagePath"
    }

    return $resolvedPackagePath
}

function Clear-NumFlowPackageDirectory {
    param(
        [Parameter(Mandatory)]
        [string]$PackagePath
    )

    $resolvedPackagePath = Resolve-NumFlowPackagePath -PackagePath $PackagePath
    if (Test-Path -LiteralPath $resolvedPackagePath) {
        $pathCursor = [System.IO.DirectoryInfo]::new($resolvedPackagePath)
        $packageRoot = [System.IO.Path]::GetFullPath((Join-Path (Get-NumFlowRepositoryRoot) 'driver\package'))
        while ($pathCursor -and $pathCursor.FullName.StartsWith(
                $packageRoot,
                [System.StringComparison]::OrdinalIgnoreCase
            )) {
            if ($pathCursor.Exists -and
                ($pathCursor.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
                throw "Refusing to clean a package path containing a reparse point: $($pathCursor.FullName)"
            }
            if ($pathCursor.FullName -eq $packageRoot) {
                break
            }
            $pathCursor = $pathCursor.Parent
        }

        $nestedReparsePoints = @(Get-ChildItem -LiteralPath $resolvedPackagePath -Force -Recurse |
            Where-Object { $_.Attributes -band [System.IO.FileAttributes]::ReparsePoint })
        if ($nestedReparsePoints.Count -ne 0) {
            throw "Refusing to clean a package directory containing reparse points: $resolvedPackagePath"
        }
        Remove-Item -LiteralPath $resolvedPackagePath -Recurse -Force
    }
    [void](New-Item -ItemType Directory -Path $resolvedPackagePath -Force)
    return $resolvedPackagePath
}

function Get-NumFlowPackageFiles {
    param(
        [Parameter(Mandatory)]
        [string]$PackagePath,

        [switch]$RequireCertificate,

        [switch]$RequireManifest
    )

    $resolvedPackagePath = Resolve-NumFlowPackagePath -PackagePath $PackagePath
    $files = [ordered]@{
        Sys = Join-Path $resolvedPackagePath 'numflow-kbd-filter.sys'
        Inf = Join-Path $resolvedPackagePath 'numflow-kbd-filter.inf'
        Cat = Join-Path $resolvedPackagePath 'numflow-kbd-filter.cat'
        Cer = Join-Path $resolvedPackagePath 'NumFlowDriverTest.cer'
        Manifest = Join-Path $resolvedPackagePath 'SHA256SUMS.txt'
    }

    foreach ($key in @('Sys', 'Inf', 'Cat')) {
        if (-not (Test-Path -LiteralPath $files[$key] -PathType Leaf)) {
            throw "Required package file is missing: $($files[$key])"
        }
    }
    if ($RequireCertificate -and -not (Test-Path -LiteralPath $files.Cer -PathType Leaf)) {
        throw "Required package certificate is missing: $($files.Cer)"
    }
    if ($RequireManifest -and -not (Test-Path -LiteralPath $files.Manifest -PathType Leaf)) {
        throw "Required package checksum manifest is missing: $($files.Manifest)"
    }

    return [pscustomobject]$files
}

function Write-NumFlowChecksumManifest {
    param(
        [Parameter(Mandatory)]
        [string]$PackagePath
    )

    $files = Get-NumFlowPackageFiles -PackagePath $PackagePath -RequireCertificate
    $content = foreach ($path in @($files.Sys, $files.Inf, $files.Cat, $files.Cer)) {
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToUpperInvariant()
        "$hash *$([System.IO.Path]::GetFileName($path))"
    }
    Set-Content -LiteralPath $files.Manifest -Value $content -Encoding ascii
    return $files.Manifest
}

function Test-NumFlowChecksumManifest {
    param(
        [Parameter(Mandatory)]
        [string]$PackagePath
    )

    $files = Get-NumFlowPackageFiles -PackagePath $PackagePath -RequireCertificate -RequireManifest
    $expectedNames = @(
        'numflow-kbd-filter.sys',
        'numflow-kbd-filter.inf',
        'numflow-kbd-filter.cat',
        'NumFlowDriverTest.cer'
    )
    $seenNames = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase
    )

    foreach ($line in Get-Content -LiteralPath $files.Manifest) {
        if ($line -notmatch '^([0-9A-Fa-f]{64}) \*([^\\/]+)$') {
            throw "Invalid SHA256SUMS.txt line: $line"
        }
        $expectedHash = $Matches[1].ToUpperInvariant()
        $name = $Matches[2]
        if ($name -notin $expectedNames -or -not $seenNames.Add($name)) {
            throw "Unexpected or duplicate checksum entry: $name"
        }
        $actualHash = (Get-FileHash -LiteralPath (Join-Path $PackagePath $name) -Algorithm SHA256).Hash
        if ($actualHash -ne $expectedHash) {
            throw "SHA-256 mismatch for $name"
        }
    }

    if ($seenNames.Count -ne $expectedNames.Count) {
        throw 'SHA256SUMS.txt does not contain all required package files.'
    }
    return $true
}

function Test-NumFlowCatalogSignatureOffline {
    param(
        [Parameter(Mandatory)]
        [string]$CatalogPath,

        [Parameter(Mandatory)]
        [string]$CertificatePath
    )

    $certificate = [System.Security.Cryptography.X509Certificates.X509Certificate2]::new(
        $CertificatePath
    )
    $contentInfo = [System.Security.Cryptography.Pkcs.ContentInfo]::new(
        [System.IO.File]::ReadAllBytes($CatalogPath)
    )
    $signedCms = [System.Security.Cryptography.Pkcs.SignedCms]::new()
    $signedCms.Decode($contentInfo.Content)
    $signedCms.CheckSignature($true)

    if ($signedCms.SignerInfos.Count -ne 1) {
        throw "Expected exactly one CAT signer; found $($signedCms.SignerInfos.Count)."
    }
    $signer = $signedCms.SignerInfos[0].Certificate
    if (-not $signer -or $signer.Thumbprint -ne $certificate.Thumbprint) {
        throw 'The CAT signer does not match NumFlowDriverTest.cer.'
    }

    $chain = [System.Security.Cryptography.X509Certificates.X509Chain]::new()
    try {
        $chain.ChainPolicy.RevocationMode =
            [System.Security.Cryptography.X509Certificates.X509RevocationMode]::NoCheck
        $chain.ChainPolicy.TrustMode =
            [System.Security.Cryptography.X509Certificates.X509ChainTrustMode]::CustomRootTrust
        [void]$chain.ChainPolicy.CustomTrustStore.Add($certificate)
        if (-not $chain.Build($signer)) {
            $statuses = $chain.ChainStatus | ForEach-Object { $_.Status.ToString() }
            throw "Offline custom-root chain validation failed: $($statuses -join ', ')"
        }
    }
    finally {
        $chain.Dispose()
        $certificate.Dispose()
    }

    return $true
}
