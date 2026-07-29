param(
    [ValidateRange(1, 20)]
    [int]$InvocationCount = 2
)

$ErrorActionPreference = "Stop"

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\release\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's release configuration before running the Delay executor smoke check."
}

$originalLocalAppData = $env:LOCALAPPDATA
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "gradial-delay-smoke-" + [guid]::NewGuid().ToString("N")
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$configPath = Join-Path $gradialDirectory "config.json"
$actionId = "67726164-6961-4c00-8000-000000000101"
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
            name = "Delay smoke"
            menu = [ordered]@{
                slice_count = 8
                slices = $slices
            }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "50 ms smoke delay"
                kind = [ordered]@{
                    type = "delay"
                    milliseconds = 50
                }
            }
        )
    }
    $json = $configuration | ConvertTo-Json -Depth 10
    [System.IO.File]::WriteAllText(
        $configPath,
        $json,
        [System.Text.UTF8Encoding]::new($false)
    )

    $env:LOCALAPPDATA = $temporaryRoot
    & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
        -Configuration release `
        -Geometry fixed-origin `
        -Slices 8 `
        -Direction E `
        -InvocationCount $InvocationCount `
        -ExpectDelayExecution
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
        (Split-Path -Leaf $resolvedTemporary.Path).StartsWith("gradial-delay-smoke-")
    ) {
        Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
    }
}
