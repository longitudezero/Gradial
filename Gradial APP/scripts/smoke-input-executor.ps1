param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("shortcut", "text")]
    [string]$Mode
)

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class GradialInputSmokeForeground
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
    throw "Build Gradial's release configuration before running the input smoke check."
}

$originalLocalAppData = $env:LOCALAPPDATA
$temporaryPrefix = "gradial-$Mode-smoke-"
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    $temporaryPrefix + [guid]::NewGuid().ToString("N")
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$configPath = Join-Path $gradialDirectory "config.json"
$eventPath = Join-Path $temporaryRoot "input-events.csv"
$readyPath = Join-Path $temporaryRoot "target-ready.txt"
$targetOutputPath = Join-Path $temporaryRoot "target-output.log"
$targetErrorPath = Join-Path $temporaryRoot "target-error.log"
$targetProcess = $null
$actionId = if ($Mode -eq "shortcut") {
    "67726164-6961-4c00-8000-000000000102"
} else {
    "67726164-6961-4c00-8000-000000000103"
}
$profileId = "67726164-6961-4c00-8000-000000000001"
$labels = @("N", "NE", "E", "SE", "S", "SW", "W", "NW")
$privacySentinel = "GRADIAL_PRIVATE_TEXT_SENTINEL"
$textValue = (
    'GRADIAL_PRIVATE_TEXT_SENTINEL %PATH% $env:PATH ' +
    [char]0x03A9 +
    " caf" +
    [char]0x00E9 +
    " " +
    [char]0xD83E +
    [char]0xDD80
)

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
    $actionKind = if ($Mode -eq "shortcut") {
        [ordered]@{
            type = "send_shortcut"
            modifiers = @("left_control")
            key = [ordered]@{
                scan_code = 100
                extended = $false
            }
        }
    } else {
        [ordered]@{
            type = "type_text"
            text = $textValue
        }
    }
    $configuration = [ordered]@{
        schema_version = 2
        config_revision = 1
        global_profile = [ordered]@{
            id = $profileId
            name = "$Mode smoke"
            menu = [ordered]@{
                slice_count = 8
                slices = $slices
            }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "$Mode smoke action"
                kind = $actionKind
            }
        )
    }
    $json = $configuration | ConvertTo-Json -Depth 10
    [System.IO.File]::WriteAllText(
        $configPath,
        $json,
        [System.Text.UTF8Encoding]::new($false)
    )

    $targetScript = Join-Path $PSScriptRoot "input-smoke-target.ps1"
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
            "The isolated input target exited with code $($targetProcess.ExitCode): " +
            $targetError
        )
    }
    if (-not (Test-Path -LiteralPath $readyPath)) {
        throw "The isolated input target did not become ready."
    }

    $ready = (Get-Content -Raw -LiteralPath $readyPath).Trim().Split(",")
    if ($ready.Count -ne 2 -or [int]$ready[0] -ne $targetProcess.Id) {
        throw "The isolated input target reported invalid identity metadata."
    }
    $targetWindow = [IntPtr]::new([long]$ready[1])
    if (-not [GradialInputSmokeForeground]::SetForegroundWindow($targetWindow)) {
        throw "Windows did not foreground the isolated input target."
    }
    Start-Sleep -Milliseconds 150
    if ([GradialInputSmokeForeground]::GetForegroundWindow() -ne $targetWindow) {
        throw "The isolated input target did not retain foreground focus."
    }

    $env:LOCALAPPDATA = $temporaryRoot
    if ($Mode -eq "shortcut") {
        & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
            -Configuration release `
            -Geometry fixed-origin `
            -Slices 8 `
            -Direction E `
            -InvocationCount 1 `
            -ExpectShortcutExecution
    }
    else {
        & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
            -Configuration release `
            -Geometry fixed-origin `
            -Slices 8 `
            -Direction E `
            -InvocationCount 1 `
            -ExpectTextExecution
    }

    Start-Sleep -Milliseconds 100
    if (-not (Test-Path -LiteralPath $eventPath)) {
        throw "The isolated target received no keyboard events."
    }
    $eventLines = @(Get-Content -LiteralPath $eventPath)
    if ($Mode -eq "shortcut") {
        $events = @(
            $eventLines |
                Where-Object { -not $_.StartsWith("char,") } |
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
    else {
        $actualUnits = @(
            $eventLines |
                Where-Object { $_.StartsWith("char,") } |
                ForEach-Object { [int]$_.Split(",")[1] }
        )
        $expectedUnits = @(
            $textValue.ToCharArray() | ForEach-Object { [int]$_ }
        )
        if ($actualUnits.Count -ne $expectedUnits.Count) {
            throw (
                "Expected $($expectedUnits.Count) Unicode character units; " +
                "observed $($actualUnits.Count)."
            )
        }
        for ($index = 0; $index -lt $expectedUnits.Count; $index++) {
            if ($actualUnits[$index] -ne $expectedUnits[$index]) {
                throw "Unicode character unit $index did not match the configured text."
            }
        }
        $logPath = Join-Path $temporaryRoot "Gradial\logs\gradial_rCURRENT.log"
        $log = Get-Content -Raw -LiteralPath $logPath
        if ($log.Contains($privacySentinel)) {
            throw "The Unicode text payload appeared in Gradial's log."
        }
        $surrogatePairs = @(
            $textValue.ToCharArray() |
                Where-Object { [char]::IsHighSurrogate($_) }
        ).Count
        $characterCount = $textValue.Length - $surrogatePairs
        if (
            $log -notmatch (
                "event_code=text_step_started .*character_count=$characterCount"
            )
        ) {
            throw "Gradial did not record the safe Unicode character count."
        }
        "Text target received the exact literal BMP and surrogate-pair payload."
    }
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
        (Split-Path -Leaf $resolvedTemporary.Path).StartsWith($temporaryPrefix)
    ) {
        Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
    }
}
