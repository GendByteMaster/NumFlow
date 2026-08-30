[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Configuration = 'Debug',

    [switch]$Rebuild
)

$ErrorActionPreference = 'Stop'

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$solutionPath = Join-Path $PSScriptRoot 'NumFlowDrivers.sln'
$msbuildPath = 'C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe'

if (-not (Test-Path -LiteralPath $msbuildPath -PathType Leaf)) {
    throw "Pinned amd64 MSBuild was not found: $msbuildPath"
}

$target = if ($Rebuild) { 'Rebuild' } else { 'Build' }
$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $msbuildPath
$startInfo.UseShellExecute = $false
$startInfo.WorkingDirectory = $repositoryRoot

# Some agent hosts expose both Path and PATH in the inherited environment. The
# .NET Framework MSBuild tool tasks treat those as duplicate case-insensitive
# keys. Copy the environment with exactly one Path entry for the child process.
foreach ($key in [Environment]::GetEnvironmentVariables().Keys) {
    if ($key -inotmatch '^Path$') {
        $startInfo.Environment[$key] = [Environment]::GetEnvironmentVariable($key)
    }
}
$startInfo.Environment['Path'] = $env:Path

foreach ($argument in @(
        $solutionPath,
        '/m',
        "/t:$target",
        "/p:Configuration=$Configuration",
        '/p:Platform=x64',
        '/v:minimal'
    )) {
    [void]$startInfo.ArgumentList.Add($argument)
}

$process = [System.Diagnostics.Process]::Start($startInfo)
$process.WaitForExit()

if ($process.ExitCode -ne 0) {
    throw "Driver build failed with exit code $($process.ExitCode)."
}
