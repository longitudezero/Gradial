param()

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class GradialWindowSmokeNative
{
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool IsIconic(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsZoomed(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsWindow(IntPtr window);
}
"@

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\release\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's release configuration before running the window-action smoke check."
}

$originalLocalAppData = $env:LOCALAPPDATA
$temporaryPrefix = "gradial-window-smoke-"
$identity = [guid]::NewGuid().ToString("N")
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    $temporaryPrefix + $identity
)
$profileId = "67726164-6961-4c00-8000-000000000001"
$labels = @("N", "NE", "E", "SE", "S", "SW", "W", "NW")
$targetScript = Join-Path $PSScriptRoot "window-smoke-target.ps1"
$commands = @(
    [pscustomobject]@{
        name = "minimize"
        initial_state = "normal"
        expected_event = "system,61472"
    },
    [pscustomobject]@{
        name = "maximize"
        initial_state = "normal"
        expected_event = "system,61488"
    },
    [pscustomobject]@{
        name = "restore"
        initial_state = "maximized"
        expected_event = "system,61728"
    },
    [pscustomobject]@{
        name = "close"
        initial_state = "normal"
        expected_event = "close"
    }
)

try {
    foreach ($command in $commands) {
        $modeRoot = Join-Path $temporaryRoot $command.name
        $gradialDirectory = Join-Path $modeRoot "Gradial"
        $configPath = Join-Path $gradialDirectory "config.json"
        $eventPath = Join-Path $modeRoot "window-events.txt"
        $readyPath = Join-Path $modeRoot "target-ready.txt"
        $targetOutputPath = Join-Path $modeRoot "target-output.log"
        $targetErrorPath = Join-Path $modeRoot "target-error.log"
        $privacySentinel = "GRADIAL_PRIVATE_WINDOW_SENTINEL_$identity"
        $windowTitle = "$privacySentinel $($command.name)"
        $targetProcess = $null
        $targetWindow = [IntPtr]::Zero
        $actionId = [guid]::NewGuid().ToString()

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
                    name = "Window $($command.name) smoke"
                    menu = [ordered]@{
                        slice_count = 8
                        slices = $slices
                    }
                }
                application_profiles = @()
                actions = @(
                    [ordered]@{
                        id = $actionId
                        name = "Window $($command.name)"
                        kind = [ordered]@{
                            type = "window"
                            command = $command.name
                        }
                    }
                )
            }
            [System.IO.File]::WriteAllText(
                $configPath,
                ($configuration | ConvertTo-Json -Depth 10),
                [System.Text.UTF8Encoding]::new($false)
            )

            $targetProcess = Start-Process -FilePath "powershell.exe" `
                -ArgumentList @(
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                    ('"' + $targetScript + '"'),
                    "-EventPath",
                    ('"' + $eventPath + '"'),
                    "-ReadyPath",
                    ('"' + $readyPath + '"'),
                    "-WindowTitle",
                    ('"' + $windowTitle + '"'),
                    "-InitialState",
                    $command.initial_state
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
                    "The $($command.name) target exited with code " +
                    "$($targetProcess.ExitCode): $targetError"
                )
            }
            if (-not (Test-Path -LiteralPath $readyPath)) {
                throw "The $($command.name) target did not become ready."
            }

            $ready = (Get-Content -Raw -LiteralPath $readyPath).Trim().Split(",")
            if ($ready.Count -ne 2 -or [int]$ready[0] -ne $targetProcess.Id) {
                throw "The $($command.name) target reported invalid identity metadata."
            }
            $targetWindow = [IntPtr]::new([long]$ready[1])
            if (-not [GradialWindowSmokeNative]::SetForegroundWindow($targetWindow)) {
                throw "Windows did not foreground the $($command.name) target."
            }
            Start-Sleep -Milliseconds 150
            if ([GradialWindowSmokeNative]::GetForegroundWindow() -ne $targetWindow) {
                throw "The $($command.name) target did not retain foreground focus."
            }

            $env:LOCALAPPDATA = $modeRoot
            & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
                -Configuration release `
                -Geometry fixed-origin `
                -Slices 8 `
                -Direction E `
                -InvocationCount 1 `
                -ExpectWindowExecution

            $effectDeadline = [DateTime]::UtcNow.AddSeconds(5)
            while ([DateTime]::UtcNow -lt $effectDeadline) {
                $events = if (Test-Path -LiteralPath $eventPath) {
                    @(Get-Content -LiteralPath $eventPath)
                } else {
                    @()
                }
                if ($events -contains $command.expected_event) {
                    break
                }
                Start-Sleep -Milliseconds 50
            }
            if ($events -notcontains $command.expected_event) {
                throw "The target did not receive the $($command.name) window command."
            }

            switch ($command.name) {
                "minimize" {
                    if (-not [GradialWindowSmokeNative]::IsIconic($targetWindow)) {
                        throw "The target did not enter the minimized state."
                    }
                }
                "maximize" {
                    if (-not [GradialWindowSmokeNative]::IsZoomed($targetWindow)) {
                        throw "The target did not enter the maximized state."
                    }
                }
                "restore" {
                    if (
                        [GradialWindowSmokeNative]::IsIconic($targetWindow) -or
                        [GradialWindowSmokeNative]::IsZoomed($targetWindow)
                    ) {
                        throw "The target did not return to its normal window state."
                    }
                }
                "close" {
                    if (-not $targetProcess.WaitForExit(3000)) {
                        throw "The polite close request did not close the target."
                    }
                    if ([GradialWindowSmokeNative]::IsWindow($targetWindow)) {
                        throw "The closed target HWND is still valid."
                    }
                }
            }

            $logPath = Join-Path $modeRoot "Gradial\logs\gradial_rCURRENT.log"
            $log = Get-Content -Raw -LiteralPath $logPath
            if ($log.Contains($privacySentinel)) {
                throw "The private target-window title appeared in Gradial's log."
            }
            if (
                $log -notmatch (
                    "event_code=window_command_step_started .*command=" +
                    $command.name
                )
            ) {
                throw "Gradial did not record the safe $($command.name) command metadata."
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
        }
    }
    "All four commands reached only their isolated invocation windows."
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
        (Split-Path -Leaf $resolvedTemporary.Path).StartsWith($temporaryPrefix)
    ) {
        Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
    }
}
