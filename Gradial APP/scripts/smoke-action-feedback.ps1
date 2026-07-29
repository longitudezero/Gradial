param()

$ErrorActionPreference = "Stop"

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\release\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's release configuration before running the action-feedback smoke check."
}

$originalLocalAppData = $env:LOCALAPPDATA
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "gradial-action-feedback-smoke-" + [guid]::NewGuid().ToString("N")
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$configPath = Join-Path $gradialDirectory "config.json"
$privateSentinel = "private-action-feedback-sentinel"
$missingExecutable = Join-Path $temporaryRoot "$privateSentinel.exe"
$actionId = "67726164-6961-4c00-8000-000000000801"
$profileId = "67726164-6961-4c00-8000-000000000001"
$labels = @("N", "NE", "E", "SE", "S", "SW", "W", "NW")

try {
    New-Item -ItemType Directory -Path $gradialDirectory -Force | Out-Null
    $slices = @(
        $labels | ForEach-Object {
            [ordered]@{
                label = $_
                action_id = $actionId
            }
        }
    )
    $configuration = [ordered]@{
        schema_version = 2
        config_revision = 1
        global_profile = [ordered]@{
            id = $profileId
            name = "Action feedback smoke"
            menu = [ordered]@{
                slice_count = 8
                slices = $slices
            }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "Missing executable feedback"
                kind = [ordered]@{
                    type = "run_executable"
                    executable = $missingExecutable
                    arguments = @("--secret-argument")
                    working_directory = $null
                }
            }
        )
    }
    [System.IO.File]::WriteAllText(
        $configPath,
        ($configuration | ConvertTo-Json -Depth 10),
        [System.Text.UTF8Encoding]::new($false)
    )

    $env:LOCALAPPDATA = $temporaryRoot
    & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
        -Configuration release `
        -Geometry fixed-origin `
        -Slices 8 `
        -Direction E `
        -InvocationCount 1 `
        -ExpectActionFailure

    $logPath = Join-Path $gradialDirectory "logs\gradial_rCURRENT.log"
    $log = Get-Content -LiteralPath $logPath -Raw
    if ($log.Contains($privateSentinel) -or $log.Contains("--secret-argument")) {
        throw "Action feedback exposed a configured path or argument in the local log."
    }
}
finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    $resolvedTemporary = Resolve-Path -LiteralPath $temporaryRoot -ErrorAction SilentlyContinue
    $resolvedSystemTemporary =
        Resolve-Path -LiteralPath ([System.IO.Path]::GetTempPath()) -ErrorAction Stop
    if (
        $resolvedTemporary -and
        $resolvedTemporary.Path.StartsWith(
            $resolvedSystemTemporary.Path,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        (Split-Path -Leaf $resolvedTemporary.Path).StartsWith(
            "gradial-action-feedback-smoke-"
        )
    ) {
        Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
    }
}
