param(
    [string]$Version = "0.3.0-beta.1",
    [switch]$SkipChecks
)

$ErrorActionPreference = "Stop"

function Assert-LastExitCode([string]$Operation) {
    if ($LASTEXITCODE -ne 0) {
        throw "$Operation failed with exit code $LASTEXITCODE."
    }
}

$workspace = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$repository = [System.IO.Path]::GetFullPath((Join-Path $workspace ".."))
$distRoot = [System.IO.Path]::GetFullPath((Join-Path $workspace "dist"))
$releaseRoot = [System.IO.Path]::GetFullPath((Join-Path $distRoot "v$Version"))
$stageName = "Gradial-$Version-Windows-x86_64"
$stageDirectory = Join-Path $releaseRoot $stageName
$portablePath = Join-Path $releaseRoot "$stageName-portable.zip"
$noticePath = Join-Path $releaseRoot "THIRD-PARTY-LICENSES.html"
$hashPath = Join-Path $releaseRoot "SHA256SUMS.txt"

if (-not $releaseRoot.StartsWith($distRoot + [System.IO.Path]::DirectorySeparatorChar)) {
    throw "The release output escaped the Gradial dist directory."
}
if ((Split-Path -Leaf $releaseRoot) -cne "v$Version") {
    throw "The release output directory does not match the requested version."
}

Push-Location $workspace
try {
    $metadata = cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json
    Assert-LastExitCode "Cargo metadata"
    $gradialPackage = @($metadata.packages | Where-Object { $_.name -eq "gradial-app" })
    if ($gradialPackage.Count -ne 1 -or $gradialPackage[0].version -cne $Version) {
        throw "Cargo package version does not match release version $Version."
    }

    if (-not $SkipChecks) {
        & (Join-Path $PSScriptRoot "check.ps1")
        Assert-LastExitCode "Project checks"
    }

    python (Join-Path $PSScriptRoot "generate-release-icon.py")
    Assert-LastExitCode "Release icon generation"

    cargo build --workspace --all-features --release --locked
    Assert-LastExitCode "Locked Release build"

    if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
        throw "cargo-about is required. Install it with: cargo install cargo-about --locked --features cli"
    }

    New-Item -ItemType Directory -Force -Path $releaseRoot | Out-Null
    cargo about generate about.hbs `
        --workspace `
        --all-features `
        --locked `
        --fail `
        --output-file $noticePath
    Assert-LastExitCode "Third-party license generation"

    if (Test-Path -LiteralPath $stageDirectory) {
        $resolvedStage = [System.IO.Path]::GetFullPath($stageDirectory)
        if (-not $resolvedStage.StartsWith($releaseRoot + [System.IO.Path]::DirectorySeparatorChar)) {
            throw "The existing staging directory escaped the release directory."
        }
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force
    }
    New-Item -ItemType Directory -Path $stageDirectory | Out-Null

    Copy-Item -LiteralPath (Join-Path $workspace "target\release\gradial.exe") -Destination $stageDirectory
    Copy-Item -LiteralPath (Join-Path $repository "LICENSE") -Destination $stageDirectory
    Copy-Item -LiteralPath (Join-Path $repository "README.md") -Destination $stageDirectory
    Copy-Item -LiteralPath (Join-Path $repository "CHANGELOG.md") -Destination $stageDirectory
    Copy-Item -LiteralPath (Join-Path $repository "SECURITY.md") -Destination $stageDirectory
    Copy-Item -LiteralPath $noticePath -Destination $stageDirectory

    $docsDirectory = Join-Path $stageDirectory "Documentation"
    New-Item -ItemType Directory -Path $docsDirectory | Out-Null
    @(
        "known-limitations-and-release-readiness.md",
        "public-beta-release-policy.md",
        "plugin-author-guide.md",
        "plugin-manifest-reference.md",
        "plugin-trust-and-capabilities-guide.md",
        "continuous-api-v1-reference.md"
    ) | ForEach-Object {
        Copy-Item -LiteralPath (Join-Path $repository "Documentation\$_") -Destination $docsDirectory
    }
    Copy-Item -LiteralPath (Join-Path $workspace "examples") -Destination $stageDirectory -Recurse

    if (Test-Path -LiteralPath $portablePath) {
        Remove-Item -LiteralPath $portablePath -Force
    }
    Compress-Archive -LiteralPath $stageDirectory -DestinationPath $portablePath -CompressionLevel Optimal

    $compilerCandidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    )
    $compiler = $compilerCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $compiler) {
        throw "Inno Setup 6 was not found."
    }

    & $compiler `
        "/DSourceDir=$stageDirectory" `
        "/DOutputDir=$releaseRoot" `
        "/DAppVersion=$Version" `
        "/DVersionInfoVersion=0.3.0.1" `
        (Join-Path $workspace "packaging\gradial.iss")
    Assert-LastExitCode "Installer build"

    $installerPath = Join-Path $releaseRoot "Gradial-$Version-Windows-x86_64-Setup.exe"
    $hashLines = @($installerPath, $portablePath) | ForEach-Object {
        $hash = Get-FileHash -LiteralPath $_ -Algorithm SHA256
        "$($hash.Hash.ToLowerInvariant())  $(Split-Path -Leaf $_)"
    }
    [System.IO.File]::WriteAllLines($hashPath, $hashLines, [System.Text.UTF8Encoding]::new($false))

    Get-Item -LiteralPath $installerPath, $portablePath, $hashPath | Select-Object Name, Length, LastWriteTimeUtc
}
finally {
    Pop-Location
}
