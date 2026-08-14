param(
    [ValidateSet("Both", "debug", "release")]
    [string]$Build = "Both",

    [string]$OutputDirectory = (
        Join-Path $PSScriptRoot "..\captures\continuous-hardening-matrix"
    ),

    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class GradialHardeningNative
{
    [StructLayout(LayoutKind.Sequential)]
    public struct Point
    {
        public int X;
        public int Y;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MouseInput
    {
        public int Dx;
        public int Dy;
        public uint MouseData;
        public uint Flags;
        public uint Time;
        public UIntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Input
    {
        public uint Type;
        public MouseInput Mouse;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct NotificationIconIdentifier
    {
        public uint Size;
        public IntPtr Window;
        public uint Id;
        public Guid Item;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    private delegate bool EnumWindowsProc(IntPtr window, IntPtr data);

    [DllImport("user32.dll")]
    private static extern uint SendInput(uint count, Input[] inputs, int size);

    [DllImport("user32.dll")]
    public static extern bool GetCursorPos(out Point point);

    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromPoint(Point point, uint flags);

    [DllImport("shcore.dll")]
    private static extern int GetDpiForMonitor(
        IntPtr monitor,
        int dpiType,
        out uint dpiX,
        out uint dpiY
    );

    [DllImport("shell32.dll")]
    private static extern int Shell_NotifyIconGetRect(
        ref NotificationIconIdentifier identifier,
        out Rect iconLocation
    );

    public static void SendM4(bool down)
    {
        SendMouse(0, 0, 1, down ? 0x0080u : 0x0100u);
    }

    public static void SendRelativeMove(int dx, int dy)
    {
        SendMouse(dx, dy, 0, 0x0001u);
    }

    public static uint EffectiveDpiAt(int x, int y)
    {
        var monitor = MonitorFromPoint(new Point { X = x, Y = y }, 2u);
        uint dpiX;
        uint dpiY;
        if (monitor == IntPtr.Zero || GetDpiForMonitor(monitor, 0, out dpiX, out dpiY) != 0) {
            return 96u;
        }
        return dpiX;
    }

    public static bool HasTrayIcon(uint processId)
    {
        var found = false;
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (windowProcessId != processId) return true;
            var identifier = new NotificationIconIdentifier {
                Size = (uint)Marshal.SizeOf(typeof(NotificationIconIdentifier)),
                Window = window,
                Id = 1,
                Item = Guid.Empty
            };
            Rect iconLocation;
            if (Shell_NotifyIconGetRect(ref identifier, out iconLocation) == 0) {
                found = true;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static bool CloseControllerWindowForProcess(uint processId)
    {
        var closed = false;
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (windowProcessId != processId) return true;
            var identifier = new NotificationIconIdentifier {
                Size = (uint)Marshal.SizeOf(typeof(NotificationIconIdentifier)),
                Window = window,
                Id = 1,
                Item = Guid.Empty
            };
            Rect iconLocation;
            if (
                Shell_NotifyIconGetRect(ref identifier, out iconLocation) == 0 &&
                PostMessage(window, 0x0010u, IntPtr.Zero, IntPtr.Zero)
            ) {
                closed = true;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return closed;
    }

    public static int PostMessageForProcess(uint processId, uint message, uint wparam)
    {
        var count = 0;
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (
                windowProcessId == processId &&
                PostMessage(window, message, new IntPtr(unchecked((long)wparam)), IntPtr.Zero)
            ) count++;
            return true;
        }, IntPtr.Zero);
        return count;
    }

    private static void SendMouse(int dx, int dy, uint data, uint flags)
    {
        var inputs = new[] {
            new Input {
                Type = 0,
                Mouse = new MouseInput {
                    Dx = dx,
                    Dy = dy,
                    MouseData = data,
                    Flags = flags,
                    Time = 0,
                    ExtraInfo = UIntPtr.Zero
                }
            }
        };
        if (SendInput(1, inputs, Marshal.SizeOf(typeof(Input))) != 1) {
            throw new InvalidOperationException("SendInput did not submit the mouse event.");
        }
    }
}
"@

function Join-ByteArrays {
    param([byte[][]]$Arrays)

    $length = ($Arrays | Measure-Object -Property Length -Sum).Sum
    $combined = New-Object byte[] $length
    $offset = 0
    foreach ($array in $Arrays) {
        [Array]::Copy($array, 0, $combined, $offset, $array.Length)
        $offset += $array.Length
    }
    return $combined
}

function Get-BoundedFieldBytes {
    param([byte[]]$Bytes)

    return Join-ByteArrays @(
        [BitConverter]::GetBytes([uint64]$Bytes.Length),
        $Bytes
    )
}

function Get-Sha256Bytes {
    param([byte[]]$Bytes)

    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return $sha256.ComputeHash($Bytes)
    }
    finally {
        $sha256.Dispose()
    }
}

function Get-PluginPackageFingerprint {
    param(
        [byte[]]$ManifestBytes,
        [byte[]]$SourceBytes
    )

    $encoding = [Text.UTF8Encoding]::new($false)
    $sourcePayload = Join-ByteArrays @(
        $encoding.GetBytes("gradial-plugin-sources-v1`0"),
        (Get-BoundedFieldBytes $encoding.GetBytes("main.lua")),
        (Get-BoundedFieldBytes $SourceBytes)
    )
    $sourceHash = Get-Sha256Bytes $sourcePayload
    $packagePayload = Join-ByteArrays @(
        $encoding.GetBytes("gradial-plugin-package-v1`0"),
        (Get-BoundedFieldBytes $ManifestBytes),
        $sourceHash
    )
    return ([BitConverter]::ToString(
        (Get-Sha256Bytes $packagePayload)
    )).Replace("-", "").ToLowerInvariant()
}

function Write-Utf8File {
    param(
        [string]$Path,
        [string]$Contents
    )

    [IO.File]::WriteAllText($Path, $Contents, [Text.UTF8Encoding]::new($false))
}

function New-HardeningFixture {
    param([string]$LocalAppData)

    $gradialDirectory = New-Item -ItemType Directory -Path (
        Join-Path $LocalAppData "Gradial"
    ) -Force
    $pluginId = "org.gradial.hardening"
    $pluginDirectory = New-Item -ItemType Directory -Path (
        Join-Path $gradialDirectory "plugins\$pluginId"
    ) -Force
    $manifest = @"
manifest_version = 1
id = "$pluginId"
name = "Continuous hardening fixture"
version = "0.1.0"
api_version = 1
entry = "main.lua"
permissions = []

[[actions]]
id = "measure"
name = "Measure"
kind = "continuous"
update_hz = 30
"@
    $source = @"
return {
    begin = function(context, parameters, initial)
        return { value = initial.value, updates = 0 }
    end,
    update = function(session, sample)
        session.value = sample.value
        session.updates = session.updates + 1
    end,
    commit = function(session, sample)
        session.value = sample.value
    end,
    cancel = function(session, reason)
        session.reason = reason
    end,
}
"@
    $manifestPath = Join-Path $pluginDirectory "plugin.toml"
    $sourcePath = Join-Path $pluginDirectory "main.lua"
    Write-Utf8File $manifestPath $manifest
    Write-Utf8File $sourcePath $source
    $fingerprint = Get-PluginPackageFingerprint `
        ([IO.File]::ReadAllBytes($manifestPath)) `
        ([IO.File]::ReadAllBytes($sourcePath))

    $pluginRecord = [ordered]@{
        enabled = $true
        approved_permissions = @()
        approved_loopback = @()
        last_loaded_manifest = [ordered]@{
            plugin_id = $pluginId
            manifest_version = 1
            plugin_version = "0.1.0"
            api_version = 1
            package_sha256 = $fingerprint
        }
        faulted = $false
    }
    $pluginRecords = [ordered]@{}
    $pluginRecords[$pluginId] = $pluginRecord
    $state = [ordered]@{
        schema_version = 1
        revision = 1
        plugins = $pluginRecords
    }
    Write-Utf8File `
        (Join-Path $gradialDirectory "plugins-state.json") `
        ($state | ConvertTo-Json -Depth 8)
    Write-Utf8File `
        (Join-Path $gradialDirectory "onboarding-v1.json") `
        '{"schema_version":1,"completed":true}'

    $continuousActionId = "67726164-6961-4c00-8000-000000000501"
    $delayActionId = "67726164-6961-4c00-8000-000000000502"
    $slices = @("N", "NE", "E", "SE", "S", "SW", "W", "NW") | ForEach-Object {
        [ordered]@{
            label = $_
            action_id = $delayActionId
            continuous_action_id = $continuousActionId
        }
    }
    $configuration = [ordered]@{
        schema_version = 6
        config_revision = 1
        activation_binding = [ordered]@{
            button = "m4"
            modifiers = [ordered]@{
                control = $false
                alt = $false
                shift = $false
            }
        }
        appearance = [ordered]@{
            accent_color = [ordered]@{ red = 124; green = 154; blue = 255 }
            label_theme = "light"
            size_preset = "standard"
        }
        continuous_full_scale_radius_dip = 300
        global_profile = [ordered]@{
            id = "67726164-6961-4c00-8000-000000000005"
            name = "Continuous hardening fixture"
            menu = [ordered]@{ slice_count = 8; slices = @($slices) }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $delayActionId
                name = "Fallback delay"
                kind = [ordered]@{ type = "delay"; milliseconds = 10 }
            },
            [ordered]@{
                id = $continuousActionId
                name = "Continuous hardening fixture"
                kind = [ordered]@{
                    type = "plugin_continuous"
                    plugin_id = $pluginId
                    plugin_action_id = "measure"
                    parameters = [ordered]@{}
                }
            }
        )
    }
    Write-Utf8File `
        (Join-Path $gradialDirectory "config.json") `
        ($configuration | ConvertTo-Json -Depth 12)

    return [pscustomobject]@{
        gradial_directory = $gradialDirectory.FullName
        plugin_id = $pluginId
        fingerprint = $fingerprint
    }
}

function Read-GradialLog {
    param([string]$GradialDirectory)

    $logDirectory = Join-Path $GradialDirectory "logs"
    if (-not (Test-Path -LiteralPath $logDirectory)) {
        return ""
    }
    return (@(
        Get-ChildItem -LiteralPath $logDirectory -Filter "gradial*.log" |
            Sort-Object LastWriteTime |
            ForEach-Object { Get-Content -Raw -LiteralPath $_.FullName }
    ) -join [Environment]::NewLine)
}

function Wait-GradialLog {
    param(
        [string]$GradialDirectory,
        [string]$Pattern,
        [int]$MinimumCount = 1,
        [int]$TimeoutSeconds = 6
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $log = Read-GradialLog $GradialDirectory
        if ([regex]::Matches($log, $Pattern).Count -ge $MinimumCount) {
            return $log
        }
        Start-Sleep -Milliseconds 20
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Timed out waiting for log pattern '$Pattern' (count $MinimumCount)."
}

function Get-LifecycleIntegrity {
    param([string]$Log)

    $pattern = (
        "event_code=plugin_lifecycle result_code=(?<result>[a-z]+) " +
        "error_code=(?<error>[a-z_]+) plugin_id=(?<plugin>[a-z0-9.-]+) " +
        "session_id=(?<session>[0-9a-f-]+) stage=(?<stage>[a-z]+)"
    )
    $records = @([regex]::Matches($Log, $pattern) | ForEach-Object {
        [pscustomobject]@{
            result = $_.Groups["result"].Value
            error = $_.Groups["error"].Value
            plugin = $_.Groups["plugin"].Value
            session = $_.Groups["session"].Value
            stage = $_.Groups["stage"].Value
        }
    })
    $invalidSessions = 0
    $updatesAfterTerminal = 0
    foreach ($group in ($records | Group-Object session)) {
        $beginCount = @($group.Group | Where-Object stage -eq "begin").Count
        $terminalIndex = -1
        $terminalCount = 0
        for ($index = 0; $index -lt $group.Group.Count; $index++) {
            $stage = $group.Group[$index].stage
            if ($stage -in @("commit", "cancel")) {
                $terminalCount++
                if ($terminalIndex -lt 0) { $terminalIndex = $index }
            }
            if ($stage -eq "update" -and $terminalIndex -ge 0) {
                $updatesAfterTerminal++
            }
        }
        if ($beginCount -ne 1 -or $terminalCount -ne 1) {
            $invalidSessions++
        }
    }
    return [pscustomobject]@{
        sessions = @($records | Group-Object session).Count
        begin = @($records | Where-Object stage -eq "begin").Count
        update = @($records | Where-Object stage -eq "update").Count
        commit = @($records | Where-Object stage -eq "commit").Count
        cancel = @($records | Where-Object stage -eq "cancel").Count
        failures = @($records | Where-Object result -ne "ok").Count
        invalid_sessions = $invalidSessions
        updates_after_terminal = $updatesAfterTerminal
    }
}

function Assert-Lifecycle {
    param(
        [object]$Integrity,
        [int]$Sessions,
        [int]$Commits,
        [int]$Cancels,
        [switch]$RequireUpdate
    )

    if (
        $Integrity.sessions -ne $Sessions -or
        $Integrity.begin -ne $Sessions -or
        $Integrity.commit -ne $Commits -or
        $Integrity.cancel -ne $Cancels -or
        $Integrity.failures -ne 0 -or
        $Integrity.invalid_sessions -ne 0 -or
        $Integrity.updates_after_terminal -ne 0 -or
        ($RequireUpdate -and $Integrity.update -eq 0)
    ) {
        throw (
            "Lifecycle integrity failed: sessions=$($Integrity.sessions), " +
            "begin=$($Integrity.begin), update=$($Integrity.update), " +
            "commit=$($Integrity.commit), cancel=$($Integrity.cancel), " +
            "failures=$($Integrity.failures), invalid=$($Integrity.invalid_sessions), " +
            "post_terminal_updates=$($Integrity.updates_after_terminal)."
        )
    }
}

function Invoke-HardeningScenario {
    param(
        [string]$Id,
        [ValidateSet("debug", "release")]
        [string]$Configuration,
        [string]$Operation,
        [int]$MonitorIndex,
        [string]$RunDirectory
    )

    $started = [DateTimeOffset]::UtcNow
    $scenarioDirectory = New-Item -ItemType Directory -Path (
        Join-Path $RunDirectory $Id
    ) -Force
    $localAppData = Join-Path $scenarioDirectory "local-appdata"
    $fixture = New-HardeningFixture $localAppData
    $executable = Join-Path $workspace "target\$Configuration\gradial.exe"
    $screen = [System.Windows.Forms.Screen]::AllScreens[$MonitorIndex]
    $originX = $screen.WorkingArea.Left + [int]($screen.WorkingArea.Width / 2)
    $originY = $screen.WorkingArea.Top + [int]($screen.WorkingArea.Height / 2)
    $dpi = [GradialHardeningNative]::EffectiveDpiAt($originX, $originY)
    # Keep the first sample comfortably beyond the 107-DIP arm threshold but
    # below full scale even when Windows pointer acceleration is enabled.
    $outsideDistance = [int][Math]::Ceiling(160.0 * $dpi / 96.0)
    $updateDistance = [int][Math]::Ceiling(30.0 * $dpi / 96.0)
    $insideDistance = [int][Math]::Round(84.0 * $dpi / 96.0)
    $originalLocalAppData = $env:LOCALAPPDATA
    $originalCursor = New-Object GradialHardeningNative+Point
    [GradialHardeningNative]::GetCursorPos([ref]$originalCursor) | Out-Null
    $process = $null
    $m4Down = $false
    $cleanExit = $false
    $errorText = ""
    $integrity = $null
    try {
        $env:LOCALAPPDATA = $localAppData
        $process = Start-Process -FilePath $executable `
            -ArgumentList @("--geometry=fixed-origin") `
            -WorkingDirectory $workspace `
            -PassThru `
            -WindowStyle Hidden
        Wait-GradialLog `
            $fixture.gradial_directory `
            "event_code=plugin_worker_ready result_code=ok .*loaded_plugins=1" | Out-Null
        Wait-GradialLog `
            $fixture.gradial_directory `
            "event_code=overlay_pool_ready result_code=ok" | Out-Null
        if (-not [GradialHardeningNative]::HasTrayIcon([uint32]$process.Id)) {
            throw "Gradial did not publish its tray icon."
        }

        function Start-ActiveGesture {
            param([switch]$RequireUpdate)

            $beforeBegins = [regex]::Matches(
                (Read-GradialLog $fixture.gradial_directory),
                "event_code=plugin_lifecycle result_code=ok .*stage=begin"
            ).Count
            [GradialHardeningNative]::SetCursorPos($originX, $originY) | Out-Null
            [GradialHardeningNative]::SendM4($true)
            $script:hardeningM4Down = $true
            Start-Sleep -Milliseconds 120
            [GradialHardeningNative]::SendRelativeMove($outsideDistance, 0)
            Wait-GradialLog `
                $fixture.gradial_directory `
                "event_code=plugin_lifecycle result_code=ok .*stage=begin" `
                ($beforeBegins + 1) | Out-Null
            if ($RequireUpdate) {
                $beforeUpdates = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=plugin_lifecycle result_code=ok .*stage=update"
                ).Count
                [GradialHardeningNative]::SendRelativeMove($updateDistance, 0)
                Wait-GradialLog `
                    $fixture.gradial_directory `
                    "event_code=plugin_lifecycle result_code=ok .*stage=update" `
                    ($beforeUpdates + 1) | Out-Null
            }
        }

        function Release-Gesture {
            [GradialHardeningNative]::SendM4($false)
            $script:hardeningM4Down = $false
            Start-Sleep -Milliseconds 80
        }

        function Invoke-CommitGesture {
            $beforeCommits = [regex]::Matches(
                (Read-GradialLog $fixture.gradial_directory),
                "event_code=plugin_lifecycle result_code=ok .*stage=commit"
            ).Count
            Start-ActiveGesture -RequireUpdate
            Release-Gesture
            Wait-GradialLog `
                $fixture.gradial_directory `
                "event_code=plugin_lifecycle result_code=ok .*stage=commit" `
                ($beforeCommits + 1) | Out-Null
        }

        $script:hardeningM4Down = $false
        switch ($Operation) {
            "commit" {
                Invoke-CommitGesture
            }
            "display-change" {
                Start-ActiveGesture -RequireUpdate
                if (
                    [GradialHardeningNative]::PostMessageForProcess(
                        [uint32]$process.Id,
                        0x007E,
                        0
                    ) -eq 0
                ) { throw "Could not post WM_DISPLAYCHANGE." }
                Wait-GradialLog $fixture.gradial_directory (
                    "event_code=invocation_cancelled .*reason=DisplayTopologyChange"
                ) | Out-Null
                Wait-GradialLog $fixture.gradial_directory (
                    "event_code=overlay_pool_ready .*generation=1"
                ) | Out-Null
                Release-Gesture
                Invoke-CommitGesture
            }
            "input-reregistration" {
                $responsiveBefore = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=hook_thread_responsive"
                ).Count
                Start-ActiveGesture -RequireUpdate
                if (
                    [GradialHardeningNative]::PostMessageForProcess(
                        [uint32]$process.Id,
                        0x0218,
                        0x12
                    ) -eq 0
                ) { throw "Could not post the resume notification." }
                Wait-GradialLog $fixture.gradial_directory (
                    "event_code=invocation_cancelled .*reason=InputReregistration"
                ) | Out-Null
                Wait-GradialLog `
                    $fixture.gradial_directory `
                    "event_code=hook_thread_responsive" `
                    ($responsiveBefore + 1) | Out-Null
                Release-Gesture
                Invoke-CommitGesture
            }
            "heartbeat" {
                $responsiveBefore = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=hook_thread_responsive"
                ).Count
                Start-ActiveGesture -RequireUpdate
                if (
                    [GradialHardeningNative]::PostMessageForProcess(
                        [uint32]$process.Id,
                        0x8247,
                        0
                    ) -eq 0
                ) { throw "Could not inject the heartbeat fault." }
                Wait-GradialLog $fixture.gradial_directory (
                    "event_code=invocation_cancelled .*reason=HookUnresponsive"
                ) | Out-Null
                Wait-GradialLog `
                    $fixture.gradial_directory `
                    "event_code=hook_thread_responsive" `
                    ($responsiveBefore + 1) | Out-Null
                Release-Gesture
                Invoke-CommitGesture
            }
            "queue-overflow" {
                $responsiveBefore = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=hook_thread_responsive"
                ).Count
                Start-ActiveGesture -RequireUpdate
                if (
                    [GradialHardeningNative]::PostMessageForProcess(
                        [uint32]$process.Id,
                        0x8248,
                        0
                    ) -eq 0
                ) { throw "Could not inject queue overflow." }
                Wait-GradialLog $fixture.gradial_directory (
                    "event_code=invocation_cancelled .*reason=InputQueueOverflow"
                ) | Out-Null
                Wait-GradialLog $fixture.gradial_directory (
                    "event_code=input_fault_requires_retry"
                ) | Out-Null
                Release-Gesture
                $invocationsBefore = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=invocation_started"
                ).Count
                [GradialHardeningNative]::SendM4($true)
                Start-Sleep -Milliseconds 80
                [GradialHardeningNative]::SendM4($false)
                Start-Sleep -Milliseconds 100
                $invocationsAfter = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=invocation_started"
                ).Count
                if ($invocationsAfter -ne $invocationsBefore) {
                    throw "Faulted input admitted an M4 invocation."
                }
                if (
                    [GradialHardeningNative]::PostMessageForProcess(
                        [uint32]$process.Id,
                        0x8249,
                        0
                    ) -eq 0
                ) { throw "Could not request Retry Input." }
                Wait-GradialLog `
                    $fixture.gradial_directory `
                    "event_code=hook_thread_responsive" `
                    ($responsiveBefore + 1) | Out-Null
                Invoke-CommitGesture
            }
            "plugin-reload" {
                Start-ActiveGesture -RequireUpdate
                if (
                    [GradialHardeningNative]::PostMessageForProcess(
                        [uint32]$process.Id,
                        0x824A,
                        0
                    ) -eq 0
                ) { throw "Could not request debug plugin reload." }
                Wait-GradialLog $fixture.gradial_directory (
                    "event_code=plugin_worker_operation result_code=ok " +
                    "operation=reload plugin_id=$($fixture.plugin_id) .*loaded=true source=request"
                ) | Out-Null
                Release-Gesture
                Invoke-CommitGesture
            }
            "plugin-disable" {
                Start-ActiveGesture -RequireUpdate
                if (
                    [GradialHardeningNative]::PostMessageForProcess(
                        [uint32]$process.Id,
                        0x824B,
                        0
                    ) -eq 0
                ) { throw "Could not request debug plugin disable." }
                Wait-GradialLog $fixture.gradial_directory (
                    "event_code=plugin_worker_operation result_code=ok " +
                    "operation=disable plugin_id=$($fixture.plugin_id) .*loaded=false source=request"
                ) | Out-Null
                Release-Gesture
                $sessionsBefore = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=live_session_started .*backend=plugin"
                ).Count
                [GradialHardeningNative]::SetCursorPos($originX, $originY) | Out-Null
                [GradialHardeningNative]::SendM4($true)
                Start-Sleep -Milliseconds 120
                [GradialHardeningNative]::SendRelativeMove($outsideDistance, 0)
                Start-Sleep -Milliseconds 100
                [GradialHardeningNative]::SendM4($false)
                Start-Sleep -Milliseconds 150
                $sessionsAfter = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=live_session_started .*backend=plugin"
                ).Count
                if ($sessionsAfter -ne $sessionsBefore) {
                    throw "A disabled plugin admitted a new continuous session."
                }
            }
            "shutdown-before-begin" {
                [GradialHardeningNative]::SetCursorPos($originX, $originY) | Out-Null
                [GradialHardeningNative]::SendM4($true)
                $script:hardeningM4Down = $true
                Wait-GradialLog $fixture.gradial_directory "event_code=invocation_started" | Out-Null
            }
            "shutdown-after-begin" {
                Start-ActiveGesture
            }
            "shutdown-after-update" {
                Start-ActiveGesture -RequireUpdate
            }
            "shutdown-at-commit" {
                Start-ActiveGesture -RequireUpdate
                Release-Gesture
            }
            "shutdown-at-cancel" {
                Start-ActiveGesture -RequireUpdate
                $beforeCancels = [regex]::Matches(
                    (Read-GradialLog $fixture.gradial_directory),
                    "event_code=plugin_lifecycle result_code=ok .*stage=cancel"
                ).Count
                $current = New-Object GradialHardeningNative+Point
                [GradialHardeningNative]::GetCursorPos([ref]$current) | Out-Null
                [GradialHardeningNative]::SendRelativeMove(
                    ($originX + $insideDistance) - $current.X,
                    $originY - $current.Y
                )
                Wait-GradialLog `
                    $fixture.gradial_directory `
                    "event_code=plugin_lifecycle result_code=ok .*stage=cancel" `
                    ($beforeCancels + 1) | Out-Null
            }
            default { throw "Unknown hardening operation '$Operation'." }
        }
        $m4Down = $script:hardeningM4Down

        if (-not [GradialHardeningNative]::CloseControllerWindowForProcess([uint32]$process.Id)) {
            throw "Could not request graceful Gradial shutdown."
        }
        if (-not $process.WaitForExit(5000)) {
            throw "Gradial exceeded the five-second campaign shutdown bound."
        }
        $process.Refresh()
        if ($process.ExitCode -ne 0) {
            throw "Gradial exited with code $($process.ExitCode)."
        }
        $cleanExit = $true
        $log = Read-GradialLog $fixture.gradial_directory
        foreach ($eventCode in @(
            "action_executor_stopped",
            "interaction_thread_stopped",
            "hook_thread_stopped",
            "plugin_worker_stopped"
        )) {
            if ($log -notmatch "event_code=$eventCode .*result_code=ok") {
                throw "Clean shutdown evidence omitted $eventCode."
            }
        }
        if ($log -match "shutdown_timeout|_panicked") {
            throw "The scenario logged a shutdown timeout or panic."
        }
        if ($log -notmatch "active_session_at_shutdown=false") {
            throw "The plugin worker retained an active session at shutdown."
        }
        $integrity = Get-LifecycleIntegrity $log
        switch ($Operation) {
            "commit" { Assert-Lifecycle $integrity 1 1 0 -RequireUpdate }
            { $_ -in @("display-change", "input-reregistration", "heartbeat", "queue-overflow", "plugin-reload") } {
                Assert-Lifecycle $integrity 2 1 1 -RequireUpdate
            }
            "plugin-disable" { Assert-Lifecycle $integrity 1 0 1 -RequireUpdate }
            "shutdown-before-begin" { Assert-Lifecycle $integrity 0 0 0 }
            { $_ -in @("shutdown-after-begin", "shutdown-after-update") } {
                Assert-Lifecycle $integrity 1 0 1 `
                    -RequireUpdate:($Operation -eq "shutdown-after-update")
            }
            "shutdown-at-commit" {
                Assert-Lifecycle `
                    $integrity `
                    1 `
                    $integrity.commit `
                    $integrity.cancel `
                    -RequireUpdate
            }
            "shutdown-at-cancel" { Assert-Lifecycle $integrity 1 0 1 -RequireUpdate }
        }
        if (
            $Operation -eq "shutdown-at-commit" -and
            ($integrity.commit + $integrity.cancel) -ne 1
        ) {
            throw "The commit/shutdown race did not produce exactly one terminal callback."
        }
    }
    catch {
        $errorText = $_.Exception.Message
    }
    finally {
        if ($script:hardeningM4Down -or $m4Down) {
            [GradialHardeningNative]::SendM4($false)
            $script:hardeningM4Down = $false
        }
        [GradialHardeningNative]::SetCursorPos($originalCursor.X, $originalCursor.Y) | Out-Null
        if ($process -and -not $process.HasExited) {
            [GradialHardeningNative]::CloseControllerWindowForProcess(
                [uint32]$process.Id
            ) | Out-Null
            if (-not $process.WaitForExit(2000)) {
                Stop-Process -Id $process.Id -Force
            }
        }
        $env:LOCALAPPDATA = $originalLocalAppData
    }

    $ended = [DateTimeOffset]::UtcNow
    $status = if ($cleanExit -and -not $errorText) { "passed" } else { "failed" }
    return [pscustomobject]@{
        id = $Id
        configuration = $Configuration
        operation = $Operation
        status = $status
        monitor_index = $MonitorIndex
        monitor_dpi = $dpi
        started_utc = $started.ToString("O")
        duration_ms = [Math]::Round(($ended - $started).TotalMilliseconds, 3)
        lifecycle_sessions = if ($integrity) { $integrity.sessions } else { $null }
        lifecycle_begins = if ($integrity) { $integrity.begin } else { $null }
        lifecycle_updates = if ($integrity) { $integrity.update } else { $null }
        lifecycle_commits = if ($integrity) { $integrity.commit } else { $null }
        lifecycle_cancels = if ($integrity) { $integrity.cancel } else { $null }
        updates_after_terminal = if ($integrity) { $integrity.updates_after_terminal } else { $null }
        error = $errorText
        evidence_directory = $scenarioDirectory.FullName
    }
}

$selfTestCatalog = @(
    "commit",
    "display-change",
    "input-reregistration",
    "heartbeat",
    "queue-overflow",
    "plugin-reload",
    "plugin-disable",
    "shutdown-before-begin",
    "shutdown-after-begin",
    "shutdown-after-update",
    "shutdown-at-commit",
    "shutdown-at-cancel"
)
if ($SelfTest) {
    $manifest = [Text.Encoding]::UTF8.GetBytes("manifest")
    $source = [Text.Encoding]::UTF8.GetBytes("source")
    $first = Get-PluginPackageFingerprint $manifest $source
    $second = Get-PluginPackageFingerprint $manifest $source
    if (
        $selfTestCatalog.Count -ne 12 -or
        $first -ne $second -or
        $first.Length -ne 64
    ) {
        throw "Continuous hardening matrix self-test failed."
    }
    Write-Host "Continuous hardening matrix self-test passed."
    return
}

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$configurations = if ($Build -eq "Both") { @("release", "debug") } else { @($Build) }
foreach ($configuration in $configurations) {
    $executable = Join-Path $workspace "target\$configuration\gradial.exe"
    if (-not (Test-Path -LiteralPath $executable)) {
        throw "Build Gradial's $configuration configuration before running the hardening matrix."
    }
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the isolated hardening matrix."
}

$runId = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$runDirectory = New-Item -ItemType Directory -Path (
    Join-Path $OutputDirectory $runId
) -Force
$startedUtc = [DateTimeOffset]::UtcNow
$screens = [System.Windows.Forms.Screen]::AllScreens
$monitors = for ($index = 0; $index -lt $screens.Count; $index++) {
    $screen = $screens[$index]
    $centerX = $screen.WorkingArea.Left + [int]($screen.WorkingArea.Width / 2)
    $centerY = $screen.WorkingArea.Top + [int]($screen.WorkingArea.Height / 2)
    [pscustomobject]@{
        index = $index
        device_name = $screen.DeviceName
        primary = $screen.Primary
        dpi = [GradialHardeningNative]::EffectiveDpiAt($centerX, $centerY)
        bounds = [pscustomobject]@{
            left = $screen.Bounds.Left
            top = $screen.Bounds.Top
            width = $screen.Bounds.Width
            height = $screen.Bounds.Height
        }
        work_area = [pscustomobject]@{
            left = $screen.WorkingArea.Left
            top = $screen.WorkingArea.Top
            width = $screen.WorkingArea.Width
            height = $screen.WorkingArea.Height
        }
    }
}

$scenarioDefinitions = [Collections.Generic.List[object]]::new()
$baselineConfiguration = if ($Build -eq "debug") { "debug" } else { "release" }
foreach ($monitor in $monitors) {
    $scenarioDefinitions.Add([pscustomobject]@{
        id = "monitor-$($monitor.index)-dpi-$($monitor.dpi)-continuous"
        configuration = $baselineConfiguration
        operation = "commit"
        monitor_index = $monitor.index
    })
}
foreach ($scenario in @(
    @{ id = "active-display-topology-change"; operation = "display-change" },
    @{ id = "active-resume-reregistration"; operation = "input-reregistration" },
    @{ id = "shutdown-before-begin"; operation = "shutdown-before-begin" },
    @{ id = "shutdown-after-begin"; operation = "shutdown-after-begin" },
    @{ id = "shutdown-after-update"; operation = "shutdown-after-update" },
    @{ id = "shutdown-at-commit"; operation = "shutdown-at-commit" },
    @{ id = "shutdown-at-cancel"; operation = "shutdown-at-cancel" }
)) {
    $scenarioDefinitions.Add([pscustomobject]@{
        id = $scenario.id
        configuration = $baselineConfiguration
        operation = $scenario.operation
        monitor_index = 0
    })
}
if ($Build -ne "release") {
    foreach ($scenario in @(
        @{ id = "active-heartbeat-recovery"; operation = "heartbeat" },
        @{ id = "active-queue-overflow-retry"; operation = "queue-overflow" },
        @{ id = "active-plugin-reload"; operation = "plugin-reload" },
        @{ id = "active-plugin-disable"; operation = "plugin-disable" }
    )) {
        $scenarioDefinitions.Add([pscustomobject]@{
            id = $scenario.id
            configuration = "debug"
            operation = $scenario.operation
            monitor_index = 0
        })
    }
}

$results = [Collections.Generic.List[object]]::new()
foreach ($scenario in $scenarioDefinitions) {
    Write-Host "[$($results.Count + 1)/$($scenarioDefinitions.Count)] $($scenario.id)"
    $result = Invoke-HardeningScenario `
        $scenario.id `
        $scenario.configuration `
        $scenario.operation `
        $scenario.monitor_index `
        $runDirectory.FullName
    $results.Add($result)
    if ($result.status -eq "failed") {
        Write-Warning "$($result.id) failed: $($result.error)"
    }
}

$dpiValues = @($monitors | ForEach-Object dpi | Sort-Object -Unique)
$coverage = [Collections.Generic.List[object]]::new()
foreach ($dpi in @(96, 120, 144, 168, 192)) {
    $scenarioIds = @($results | Where-Object monitor_dpi -eq $dpi | ForEach-Object id)
    $status = if ($scenarioIds.Count -eq 0) {
        "unavailable"
    } elseif (@($results | Where-Object id -in $scenarioIds | Where-Object status -eq "failed").Count) {
        "failed"
    } else {
        "passed"
    }
    $coverage.Add([pscustomobject]@{
        requirement = "continuous route at $dpi DPI"
        status = $status
        evidence = if ($status -eq "unavailable") {
            "No active monitor reported $dpi effective DPI."
        } else {
            "Production hook, overlay, scalar, Lua Begin/Update/Commit, and shutdown exercised on: $($scenarioIds -join ', ')."
        }
    })
}
foreach ($entry in @(
    @{ requirement = "display topology cancellation and pool rebuild"; id = "active-display-topology-change" },
    @{ requirement = "resume re-registration during an active session"; id = "active-resume-reregistration" },
    @{ requirement = "heartbeat automatic recovery during an active session"; id = "active-heartbeat-recovery" },
    @{ requirement = "queue-overflow pass-through and manual retry"; id = "active-queue-overflow-retry" },
    @{ requirement = "plugin reload cancels before operation"; id = "active-plugin-reload" },
    @{ requirement = "plugin disable cancels and revokes admission"; id = "active-plugin-disable" }
)) {
    $record = $results | Where-Object id -eq $entry.id | Select-Object -First 1
    $coverage.Add([pscustomobject]@{
        requirement = $entry.requirement
        status = if ($record) { $record.status } else { "not_run" }
        evidence = if ($record) { $record.id } else { "Excluded by the selected build set." }
    })
}
$shutdownIds = @(
    "shutdown-before-begin",
    "shutdown-after-begin",
    "shutdown-after-update",
    "shutdown-at-commit",
    "shutdown-at-cancel"
)
$shutdownRecords = @($results | Where-Object id -in $shutdownIds)
$coverage.Add([pscustomobject]@{
    requirement = "bounded shutdown across continuous lifecycle phases"
    status = if (@($shutdownRecords | Where-Object status -eq "failed").Count) {
        "failed"
    } elseif ($shutdownRecords.Count -eq $shutdownIds.Count) {
        "passed"
    } else {
        "not_run"
    }
    evidence = "Before Begin, after Begin, after Update, at Commit submission, and after Cancel submission; exact one-terminal integrity is required."
})
foreach ($requirement in @(
    "physical monitor attach/detach",
    "physical DPI change during an active session",
    "physical sleep/resume",
    "session lock/unlock",
    "Explorer restart"
)) {
    $coverage.Add([pscustomobject]@{
        requirement = $requirement
        status = "not_run"
        evidence = "Requires an external physical or Windows-shell state transition; the deterministic notification route is covered where available."
    })
}

$endedUtc = [DateTimeOffset]::UtcNow
$summary = [ordered]@{
    scenarios = $results.Count
    passed = @($results | Where-Object status -eq "passed").Count
    failed = @($results | Where-Object status -eq "failed").Count
    observed_dpi = $dpiValues
    lifecycle_sessions = ($results | Measure-Object lifecycle_sessions -Sum).Sum
    lifecycle_updates = ($results | Measure-Object lifecycle_updates -Sum).Sum
    coverage_passed = @($coverage | Where-Object status -eq "passed").Count
    coverage_failed = @($coverage | Where-Object status -eq "failed").Count
    coverage_unavailable = @($coverage | Where-Object status -eq "unavailable").Count
    coverage_not_run = @($coverage | Where-Object status -eq "not_run").Count
}
$report = [ordered]@{
    schema_version = 1
    run_id = $runId
    build = $Build
    started_utc = $startedUtc.ToString("O")
    ended_utc = $endedUtc.ToString("O")
    duration_ms = [Math]::Round(($endedUtc - $startedUtc).TotalMilliseconds, 3)
    host = [ordered]@{
        computer_name = $env:COMPUTERNAME
        operating_system = [Environment]::OSVersion.VersionString
        monitors = $monitors
    }
    summary = $summary
    scenarios = $results
    coverage = $coverage
}
$jsonPath = Join-Path $runDirectory "continuous-hardening-matrix.json"
$scenarioCsvPath = Join-Path $runDirectory "scenarios.csv"
$coverageCsvPath = Join-Path $runDirectory "coverage.csv"
Write-Utf8File $jsonPath ($report | ConvertTo-Json -Depth 12)
$results | Export-Csv -LiteralPath $scenarioCsvPath -NoTypeInformation -Encoding utf8
$coverage | Export-Csv -LiteralPath $coverageCsvPath -NoTypeInformation -Encoding utf8

Write-Host ""
Write-Host "Continuous hardening matrix: $($summary.passed) passed, $($summary.failed) failed."
Write-Host "Observed DPI: $($dpiValues -join ', ')"
Write-Host "JSON report:    $jsonPath"
Write-Host "Scenario table: $scenarioCsvPath"
Write-Host "Coverage table: $coverageCsvPath"
if ($summary.failed -gt 0) {
    exit 1
}
