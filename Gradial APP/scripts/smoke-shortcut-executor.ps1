param()

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class GradialShortcutSmokeForeground
{
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
}
"@

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\release\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's release configuration before running the shortcut smoke check."
}

$originalLocalAppData = $env:LOCALAPPDATA
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "gradial-shortcut-smoke-" + [guid]::NewGuid().ToString("N")
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$configPath = Join-Path $gradialDirectory "config.json"
$eventPath = Join-Path $temporaryRoot "keyboard-events.csv"
$readyPath = Join-Path $temporaryRoot "target-ready.txt"
$targetOutputPath = Join-Path $temporaryRoot "target-output.log"
$targetErrorPath = Join-Path $temporaryRoot "target-error.log"
$targetProcess = $null
$actionId = "67726164-6961-4c00-8000-000000000102"
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
            name = "Shortcut smoke"
            menu = [ordered]@{
                slice_count = 8
                slices = $slices
            }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "Ctrl+F13 smoke shortcut"
                kind = [ordered]@{
                    type = "send_shortcut"
                    modifiers = @("left_control")
                    key = [ordered]@{
                        scan_code = 100
                        extended = $false
                    }
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

    $targetScript = Join-Path $PSScriptRoot "shortcut-smoke-target.ps1"
    $quotedTargetScript = '"' + $targetScript + '"'
    $quotedEventPath = '"' + $eventPath + '"'
    $quotedReadyPath = '"' + $readyPath + '"'
    $targetProcess = Start-Process -FilePath "powershell.exe" `
        -ArgumentList @(
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            $quotedTargetScript,
            "-EventPath",
            $quotedEventPath,
            "-ReadyPath",
            $quotedReadyPath
        ) `
        -RedirectStandardOutput $targetOutputPath `
        -RedirectStandardError $targetErrorPath `
        -PassThru

    $readyDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while (
        -not (Test-Path -LiteralPath $readyPath) -and
        -not $targetProcess.HasExited -and
        [DateTime]::UtcNow -lt $readyDeadline
    ) {
        Start-Sleep -Milliseconds 50
    }
    if ($targetProcess.HasExited) {
        $targetProcess.Refresh()
        $targetError = if (Test-Path -LiteralPath $targetErrorPath) {
            (Get-Content -Raw -LiteralPath $targetErrorPath).Trim()
        } else {
            "no error output"
        }
        throw (
            "The isolated shortcut target exited with code $($targetProcess.ExitCode): " +
            $targetError
        )
    }
    if (-not (Test-Path -LiteralPath $readyPath)) {
        throw "The isolated shortcut target did not become ready."
    }

    $ready = (Get-Content -Raw -LiteralPath $readyPath).Trim().Split(",")
    if ($ready.Count -ne 2 -or [int]$ready[0] -ne $targetProcess.Id) {
        throw "The isolated shortcut target reported invalid identity metadata."
    }
    $targetWindow = [IntPtr]::new([long]$ready[1])
    if (-not [GradialShortcutSmokeForeground]::SetForegroundWindow($targetWindow)) {
        throw "Windows did not foreground the isolated shortcut target."
    }
    Start-Sleep -Milliseconds 150
    if ([GradialShortcutSmokeForeground]::GetForegroundWindow() -ne $targetWindow) {
        throw "The isolated shortcut target did not retain foreground focus."
    }

    $env:LOCALAPPDATA = $temporaryRoot
    & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
        -Configuration release `
        -Geometry fixed-origin `
        -Slices 8 `
        -Direction E `
        -InvocationCount 1 `
        -ExpectShortcutExecution

    Start-Sleep -Milliseconds 100
    if (-not (Test-Path -LiteralPath $eventPath)) {
        throw "The isolated target received no keyboard events."
    }
    $events = @(
        Get-Content -LiteralPath $eventPath |
            ForEach-Object {
                $fields = $_.Split(",")
                [pscustomobject]@{
                    message = [int]$fields[0]
                    virtual_key = [int]$fields[1]
                    scan_code = [int]$fields[2]
                    extended = [bool]::Parse($fields[3])
                    key_up = [bool]::Parse($fields[4])
                }
            }
    )
    $expected = @(
        [pscustomobject]@{ scan_code = 29; extended = $false; key_up = $false },
        [pscustomobject]@{ scan_code = 100; extended = $false; key_up = $false },
        [pscustomobject]@{ scan_code = 100; extended = $false; key_up = $true },
        [pscustomobject]@{ scan_code = 29; extended = $false; key_up = $true }
    )
    if ($events.Count -ne $expected.Count) {
        throw "Expected four shortcut key events; observed $($events.Count)."
    }
    for ($index = 0; $index -lt $expected.Count; $index++) {
        if (
            $events[$index].scan_code -ne $expected[$index].scan_code -or
            $events[$index].extended -ne $expected[$index].extended -or
            $events[$index].key_up -ne $expected[$index].key_up
        ) {
            throw "Shortcut event $index did not match the Ctrl+F13 cleanup contract."
        }
    }
    "Shortcut target received Ctrl down, F13 down/up, and Ctrl up in order."
}
finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    if ($targetProcess -and -not $targetProcess.HasExited) {
        [void]$targetProcess.CloseMainWindow()
        if (-not $targetProcess.WaitForExit(3000)) {
            Stop-Process -Id $targetProcess.Id -Force
        }
    }
    $resolvedTemporary = Resolve-Path -LiteralPath $temporaryRoot -ErrorAction SilentlyContinue
    $resolvedSystemTemporary =
        Resolve-Path -LiteralPath ([System.IO.Path]::GetTempPath()) -ErrorAction Stop
    if (
        $resolvedTemporary -and
        $resolvedTemporary.Path.StartsWith(
            $resolvedSystemTemporary.Path,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        (Split-Path -Leaf $resolvedTemporary.Path).StartsWith("gradial-shortcut-smoke-")
    ) {
        Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
    }
}
