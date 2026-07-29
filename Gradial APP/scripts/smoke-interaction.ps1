param(
    [ValidateSet("auto", "fixed-origin", "edge-assist")]
    [string]$Geometry = "auto",

    [ValidateSet("4", "8")]
    [string]$Slices = "8",

    [ValidateSet("debug", "release")]
    [string]$Configuration = "debug",

    [switch]$CaptureTiming,

    [switch]$TimingMarkersOnly,

    [ValidateRange(1, 30)]
    [int]$CaptureDurationSeconds = 4,

    [ValidateRange(0, 15)]
    [int]$MonitorIndex = 0,

    [switch]$SimulateDisplayChange,

    [switch]$SimulateInputReregistration,

    [ValidateSet("none", "heartbeat", "queue-overflow")]
    [string]$SimulateInputFault = "none",

    [switch]$ReducedMotion,

    [switch]$QuickFlick,

    [ValidateSet(
        "default",
        "center",
        "top-edge",
        "right-edge",
        "bottom-edge",
        "left-edge",
        "top-left",
        "top-right",
        "bottom-right",
        "bottom-left"
    )]
    [string]$ActivationPoint = "default",

    [ValidateSet("N", "NE", "E", "SE", "S", "SW", "W", "NW")]
    [string]$Direction = "E",

    [switch]$AllDirections,

    [ValidateRange(1, 1000)]
    [int]$InvocationCount = 1,

    [ValidateRange(0, 1000)]
    [int]$GestureDelayMilliseconds = 180,

    [ValidateRange(0, 1000)]
    [int]$InterInvocationDelayMilliseconds = 75
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

public static class GradialSmokeNative
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
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);

    [DllImport("shell32.dll")]
    private static extern int Shell_NotifyIconGetRect(
        ref NotificationIconIdentifier identifier,
        out Rect iconLocation
    );

    public static uint ForegroundProcessId()
    {
        uint processId;
        GetWindowThreadProcessId(GetForegroundWindow(), out processId);
        return processId;
    }

    public static void SendM4(bool down)
    {
        SendMouse(0, 0, 1, down ? 0x0080u : 0x0100u);
    }

    public static void SendRelativeMove(int dx, int dy)
    {
        SendMouse(dx, dy, 0, 0x0001u);
    }

    public static void CloseWindowsForProcess(uint processId)
    {
        var windows = new List<IntPtr>();
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (windowProcessId == processId) windows.Add(window);
            return true;
        }, IntPtr.Zero);
        foreach (var window in windows) PostMessage(window, 0x0010u, IntPtr.Zero, IntPtr.Zero);
    }

    public static int NotifyDisplayChangeForProcess(uint processId)
    {
        return PostMessageForProcess(processId, 0x007Eu, UIntPtr.Zero);
    }

    public static int NotifyResumeForProcess(uint processId)
    {
        return PostMessageForProcess(processId, 0x0218u, new UIntPtr(0x12u));
    }

    public static int SimulateInputFaultForProcess(uint processId, bool queueOverflow)
    {
        return PostMessageForProcess(
            processId,
            queueOverflow ? 0x8248u : 0x8247u,
            UIntPtr.Zero
        );
    }

    public static int RetryInputForProcess(uint processId)
    {
        return PostMessageForProcess(processId, 0x8249u, UIntPtr.Zero);
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

    private static int PostMessageForProcess(
        uint processId,
        uint message,
        UIntPtr wparam
    )
    {
        var count = 0;
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (windowProcessId == processId && PostMessage(
                window,
                message,
                new IntPtr(unchecked((long)wparam.ToUInt64())),
                IntPtr.Zero
            )) count++;
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

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\$Configuration\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial before running the live smoke check."
}
if ($Configuration -ne "debug" -and $SimulateInputFault -ne "none") {
    throw "Input fault simulation is available only in a debug build."
}
if ($Slices -eq "4" -and -not $AllDirections -and $Direction.Length -ne 1) {
    throw "Four-slice validation accepts only N, E, S, or W."
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the isolated live smoke check."
}

$originalCursor = New-Object GradialSmokeNative+Point
if (-not [GradialSmokeNative]::GetCursorPos([ref]$originalCursor)) {
    throw "Could not capture the original cursor position."
}
$foregroundWindow = [GradialSmokeNative]::GetForegroundWindow()
$foregroundProcess = [GradialSmokeNative]::ForegroundProcessId()
$m4IsDown = $false
$gradial = $null
$timingCapture = $null

try {
    $screens = [System.Windows.Forms.Screen]::AllScreens
    if ($MonitorIndex -ge $screens.Count) {
        throw "Monitor index $MonitorIndex is unavailable; Windows reported $($screens.Count) display(s)."
    }
    $workArea = $screens[$MonitorIndex].WorkingArea
    $resolvedActivationPoint = if ($ActivationPoint -eq "default") {
        if ($Geometry -eq "fixed-origin") { "center" } else { "top-left" }
    } else {
        $ActivationPoint
    }
    $centerX = $workArea.Left + [int]($workArea.Width / 2)
    $centerY = $workArea.Top + [int]($workArea.Height / 2)
    switch ($resolvedActivationPoint) {
        "center" {
            $testX = $centerX
            $testY = $centerY
        }
        "top-edge" {
            $testX = $centerX
            $testY = $workArea.Top + 2
        }
        "right-edge" {
            $testX = $workArea.Right - 3
            $testY = $centerY
        }
        "bottom-edge" {
            $testX = $centerX
            $testY = $workArea.Bottom - 3
        }
        "left-edge" {
            $testX = $workArea.Left + 2
            $testY = $centerY
        }
        "top-left" {
            $testX = $workArea.Left + 2
            $testY = $workArea.Top + 2
        }
        "top-right" {
            $testX = $workArea.Right - 3
            $testY = $workArea.Top + 2
        }
        "bottom-right" {
            $testX = $workArea.Right - 3
            $testY = $workArea.Bottom - 3
        }
        "bottom-left" {
            $testX = $workArea.Left + 2
            $testY = $workArea.Bottom - 3
        }
    }
    $directionVectors = @{
        N  = @(0, -80)
        NE = @(57, -57)
        E  = @(80, 0)
        SE = @(57, 57)
        S  = @(0, 80)
        SW = @(-57, 57)
        W  = @(-80, 0)
        NW = @(-57, -57)
    }
    $directionSlices = if ($Slices -eq "4") {
        @{ N = 0; E = 1; S = 2; W = 3 }
    } else {
        @{ N = 0; NE = 1; E = 2; SE = 3; S = 4; SW = 5; W = 6; NW = 7 }
    }
    $directions = if ($AllDirections) {
        if ($Slices -eq "4") { @("N", "E", "S", "W") }
        else { @("N", "NE", "E", "SE", "S", "SW", "W", "NW") }
    } else {
        @($Direction)
    }
    if (-not [GradialSmokeNative]::SetCursorPos($testX, $testY)) {
        throw "Could not position the cursor for the smoke check."
    }
    $cursorAtActivation = New-Object GradialSmokeNative+Point
    if (-not [GradialSmokeNative]::GetCursorPos([ref]$cursorAtActivation)) {
        throw "Could not read the positioned cursor."
    }

    $startupArguments = @("--geometry=$Geometry")
    if ($Configuration -eq "debug") {
        $startupArguments += "--slices=$Slices"
    }
    if ($ReducedMotion) {
        $startupArguments += "--reduced-motion"
    }
    $gradial = Start-Process -FilePath $executable `
        -ArgumentList $startupArguments `
        -WorkingDirectory $workspace `
        -PassThru `
        -WindowStyle Hidden
    Start-Sleep -Milliseconds 1200
    if ($gradial.HasExited) {
        throw "Gradial exited during startup with code $($gradial.ExitCode)."
    }
    if (-not [GradialSmokeNative]::HasTrayIcon([uint32]$gradial.Id)) {
        throw "Gradial did not register its notification-area icon."
    }
    $logPath = Join-Path $env:LOCALAPPDATA "Gradial\logs\gradial_rCURRENT.log"
    $startupLog = Get-Content -Raw -LiteralPath $logPath
    $responsiveBeforeRecovery = (
        [regex]::Matches($startupLog, "event_code=hook_thread_responsive")
    ).Count

    if ($SimulateInputReregistration) {
        $reregistrationBefore = (
            [regex]::Matches($startupLog, "event_code=input_reregistration_requested")
        ).Count
        $notifiedWindows = [GradialSmokeNative]::NotifyResumeForProcess(
            [uint32]$gradial.Id
        )
        if ($notifiedWindows -eq 0) {
            throw "Could not deliver a resume notification to Gradial."
        }
        $recoveryDeadline = [DateTime]::UtcNow.AddSeconds(4)
        do {
            Start-Sleep -Milliseconds 100
            $recoveryLog = Get-Content -Raw -LiteralPath $logPath
            $responsiveAfterRecovery = (
                [regex]::Matches($recoveryLog, "event_code=hook_thread_responsive")
            ).Count
            $reregistrationAfter = (
                [regex]::Matches($recoveryLog, "event_code=input_reregistration_requested")
            ).Count
        } while (
            [DateTime]::UtcNow -lt $recoveryDeadline -and
            (
                $responsiveAfterRecovery -le $responsiveBeforeRecovery -or
                $reregistrationAfter -le $reregistrationBefore
            )
        )
        if (
            $responsiveAfterRecovery -le $responsiveBeforeRecovery -or
            $reregistrationAfter -le $reregistrationBefore
        ) {
            throw "Gradial did not re-register input after the resume notification."
        }
        $responsiveBeforeRecovery = $responsiveAfterRecovery
        $startupLog = $recoveryLog
    }

    if ($SimulateInputFault -ne "none") {
        $faultPattern = if ($SimulateInputFault -eq "heartbeat") {
            "event_code=hook_thread_unresponsive"
        }
        else {
            "event_code=input_fault_requires_retry"
        }
        $faultCountBefore = ([regex]::Matches($startupLog, $faultPattern)).Count
        $queueOverflow = $SimulateInputFault -eq "queue-overflow"
        $notifiedWindows = [GradialSmokeNative]::SimulateInputFaultForProcess(
            [uint32]$gradial.Id,
            $queueOverflow
        )
        if ($notifiedWindows -eq 0) {
            throw "Could not deliver the debug input-fault notification to Gradial."
        }
        $faultDeadline = [DateTime]::UtcNow.AddSeconds(4)
        do {
            Start-Sleep -Milliseconds 100
            $faultLog = Get-Content -Raw -LiteralPath $logPath
            $faultCountAfter = ([regex]::Matches($faultLog, $faultPattern)).Count
            $responsiveAfterRecovery = (
                [regex]::Matches($faultLog, "event_code=hook_thread_responsive")
            ).Count
            $automaticRecoveryComplete = (
                $SimulateInputFault -eq "heartbeat" -and
                $responsiveAfterRecovery -gt $responsiveBeforeRecovery
            )
            $manualFaultReached = (
                $SimulateInputFault -eq "queue-overflow" -and
                $faultCountAfter -gt $faultCountBefore
            )
        } while (
            [DateTime]::UtcNow -lt $faultDeadline -and
            (
                $faultCountAfter -le $faultCountBefore -or
                (-not $automaticRecoveryComplete -and -not $manualFaultReached)
            )
        )
        if ($faultCountAfter -le $faultCountBefore) {
            throw "Gradial did not report the simulated $SimulateInputFault fault."
        }
        if ($SimulateInputFault -eq "heartbeat") {
            if (-not $automaticRecoveryComplete) {
                throw "Gradial did not automatically restart input after the heartbeat fault."
            }
        }
        else {
            $invocationsBeforePassThrough = (
                [regex]::Matches($faultLog, "event_code=invocation_started")
            ).Count
            [GradialSmokeNative]::SendM4($true)
            $m4IsDown = $true
            Start-Sleep -Milliseconds 100
            [GradialSmokeNative]::SendM4($false)
            $m4IsDown = $false
            Start-Sleep -Milliseconds 150
            $passThroughLog = Get-Content -Raw -LiteralPath $logPath
            $invocationsAfterPassThrough = (
                [regex]::Matches($passThroughLog, "event_code=invocation_started")
            ).Count
            if ($invocationsAfterPassThrough -ne $invocationsBeforePassThrough) {
                throw "Gradial accepted M4 while its input health was faulted."
            }
            if (
                [GradialSmokeNative]::RetryInputForProcess([uint32]$gradial.Id) -eq 0
            ) {
                throw "Could not deliver the debug Retry Input command to Gradial."
            }
            $retryDeadline = [DateTime]::UtcNow.AddSeconds(4)
            do {
                Start-Sleep -Milliseconds 100
                $retryLog = Get-Content -Raw -LiteralPath $logPath
                $responsiveAfterRecovery = (
                    [regex]::Matches($retryLog, "event_code=hook_thread_responsive")
                ).Count
            } while (
                [DateTime]::UtcNow -lt $retryDeadline -and
                $responsiveAfterRecovery -le $responsiveBeforeRecovery
            )
            if ($responsiveAfterRecovery -le $responsiveBeforeRecovery) {
                throw "Retry Input did not restart input after queue overflow."
            }
        }
    }
    if ($SimulateDisplayChange) {
        [GradialSmokeNative]::SendM4($true)
        $m4IsDown = $true
        Start-Sleep -Milliseconds 180
        $notifiedWindows = [GradialSmokeNative]::NotifyDisplayChangeForProcess(
            [uint32]$gradial.Id
        )
        if ($notifiedWindows -eq 0) {
            throw "Could not deliver a display-change notification to Gradial."
        }

        $rebuildDeadline = [DateTime]::UtcNow.AddSeconds(4)
        $poolRebuilt = $false
        $activeGestureCancelled = $false
        while ([DateTime]::UtcNow -lt $rebuildDeadline) {
            if (Test-Path -LiteralPath $logPath) {
                $logContents = Get-Content -Raw -LiteralPath $logPath
                $poolRebuilt = $logContents -match `
                    "event_code=overlay_pool_ready.*generation=1"
                $activeGestureCancelled = $logContents -match `
                    "event_code=invocation_cancelled.*reason=DisplayTopologyChange"
                if ($poolRebuilt -and $activeGestureCancelled) {
                    break
                }
            }
            Start-Sleep -Milliseconds 100
        }
        if (-not $poolRebuilt) {
            throw "Gradial did not rebuild and rewarm its overlay pool after the display change."
        }
        if (-not $activeGestureCancelled) {
            throw "Gradial did not cancel the active gesture during the display change."
        }
        if ($gradial.HasExited) {
            throw "Gradial exited while rebuilding its overlay pool."
        }
        [GradialSmokeNative]::SendM4($false)
        $m4IsDown = $false
        Start-Sleep -Milliseconds 100
    }
    if ($CaptureTiming) {
        $captureScript = Join-Path $PSScriptRoot "capture-timing.ps1"
        $captureArguments = "-NoProfile -ExecutionPolicy Bypass -File `"$captureScript`" " +
            "-DurationSeconds $CaptureDurationSeconds"
        if ($TimingMarkersOnly) {
            $captureArguments += " -MarkersOnly"
        }
        $timingCapture = Start-Process -FilePath "powershell.exe" `
            -ArgumentList $captureArguments `
            -WorkingDirectory $workspace `
            -PassThru `
            -WindowStyle Hidden
        Start-Sleep -Milliseconds 900
        if ($timingCapture.HasExited) {
            throw "The timing capture helper exited before the gesture with code $($timingCapture.ExitCode)."
        }
    }

    # Recovery pass-through checks may legitimately let the foreground
    # application handle M4. Establish the focus baseline immediately before
    # the measured Gradial invocation set so only Gradial-caused changes fail.
    $foregroundWindow = [GradialSmokeNative]::GetForegroundWindow()
    $foregroundProcess = [GradialSmokeNative]::ForegroundProcessId()
    $gestureDelay = if ($QuickFlick) { 0 } else { $GestureDelayMilliseconds }
    $maxCursorDistance = 0.0
    $completedInvocations = 0
    foreach ($gestureDirection in $directions) {
        $vector = $directionVectors[$gestureDirection]
        for ($invocationIndex = 0; $invocationIndex -lt $InvocationCount; $invocationIndex++) {
            if (-not [GradialSmokeNative]::SetCursorPos($testX, $testY)) {
                throw "Could not reset the cursor for invocation $invocationIndex."
            }
            $cursorAtActivation = New-Object GradialSmokeNative+Point
            [GradialSmokeNative]::GetCursorPos([ref]$cursorAtActivation) | Out-Null

            [GradialSmokeNative]::SendM4($true)
            $m4IsDown = $true
            if ($gestureDelay -gt 0) {
                Start-Sleep -Milliseconds $gestureDelay
            }
            [GradialSmokeNative]::SendRelativeMove($vector[0], $vector[1])
            if ($gestureDelay -gt 0) {
                Start-Sleep -Milliseconds $gestureDelay
            }

            $cursorDuringGesture = New-Object GradialSmokeNative+Point
            [GradialSmokeNative]::GetCursorPos([ref]$cursorDuringGesture) | Out-Null
            $cursorDistance = [Math]::Sqrt(
                [Math]::Pow($cursorDuringGesture.X - $cursorAtActivation.X, 2) +
                [Math]::Pow($cursorDuringGesture.Y - $cursorAtActivation.Y, 2)
            )
            $maxCursorDistance = [Math]::Max($maxCursorDistance, $cursorDistance)
            [GradialSmokeNative]::SendM4($false)
            $m4IsDown = $false
            $completedInvocations++
            if (
                $completedInvocations -lt ($directions.Count * $InvocationCount) -and
                $InterInvocationDelayMilliseconds -gt 0
            ) {
                Start-Sleep -Milliseconds $InterInvocationDelayMilliseconds
            }
        }
    }
    Start-Sleep -Milliseconds 250

    $foregroundAfter = [GradialSmokeNative]::GetForegroundWindow()
    $foregroundProcessAfter = [GradialSmokeNative]::ForegroundProcessId()
    if ($foregroundAfter -ne $foregroundWindow -and $foregroundProcessAfter -ne $foregroundProcess) {
        throw "The foreground application changed during the Gradial invocation."
    }
    $expectedBehavior = if ($Geometry -eq "fixed-origin") {
        "fixed-origin"
    } elseif ($Geometry -eq "edge-assist" -or $resolvedActivationPoint -ne "center") {
        "edge-assist"
    } else {
        "fixed-origin"
    }
    if ($expectedBehavior -eq "edge-assist") {
        if ($maxCursorDistance -gt 2) {
            throw "Edge assist moved the system cursor by $maxCursorDistance pixels."
        }
    }
    if ($timingCapture) {
        $timingCapture.WaitForExit()
        if ($timingCapture.ExitCode -ne 0) {
            throw "The timing capture helper exited with code $($timingCapture.ExitCode)."
        }
    }
}
finally {
    if ($m4IsDown) {
        [GradialSmokeNative]::SendM4($false)
    }
    [GradialSmokeNative]::SetCursorPos($originalCursor.X, $originalCursor.Y) | Out-Null
    if ($gradial -and -not $gradial.HasExited) {
        [GradialSmokeNative]::CloseWindowsForProcess([uint32]$gradial.Id)
        if (-not $gradial.WaitForExit(5000)) {
            Stop-Process -Id $gradial.Id -Force
        }
    }
    if ($timingCapture -and -not $timingCapture.HasExited) {
        if (-not $timingCapture.WaitForExit(($CaptureDurationSeconds + 5) * 1000)) {
            Stop-Process -Id $timingCapture.Id -Force
        }
    }
}

$logPath = Join-Path $env:LOCALAPPDATA "Gradial\logs\gradial_rCURRENT.log"
$log = Get-Content -LiteralPath $logPath -Raw
$configurationRecord = [regex]::Match(
    $log,
    "event_code=config_loaded .*config_revision=(?<revision>[1-9][0-9]*) " +
    "schema_version=2 source=(?<source>[a-z_]+) " +
    "migrated_from_schema=(?<migration>none|1) action_count=(?<actions>[0-9]+)"
)
if (-not $configurationRecord.Success) {
    throw "The live check did not load a validated schema-two configuration."
}
$activeConfigRevision = $configurationRecord.Groups["revision"].Value
$expectedDebugOverride = if ($Configuration -eq "debug") { $Slices } else { "none" }
if (
    $log -notmatch (
        "event_code=input_geometry_ready .*geometry_mode=$Geometry " +
        ".*debug_slice_override=$expectedDebugOverride"
    )
) {
    throw "The log does not confirm geometry mode '$Geometry' and the expected menu authority."
}
$expectedReducedMotion = if ($ReducedMotion) { "true" } else { "false" }
if (
    $log -notmatch (
        "event_code=renderer_geometry_cache_ready .*slice_counts=4,8 " +
        ".*reduced_motion=$expectedReducedMotion .*open_ms=60 " +
        ".*selection_ms=35 .*close_ms=50"
    )
) {
    throw "The renderer cache and animation contract was not initialized as requested."
}
$expectedInvocations = $directions.Count * $InvocationCount
$snapshotRecords = [regex]::Matches(
    $log,
    "event_code=invocation_snapshot_captured .*" +
    "config_revision=$activeConfigRevision .*activation_binding=MouseButton4"
)
if ($snapshotRecords.Count -lt $expectedInvocations) {
    throw "Expected $expectedInvocations complete invocation snapshots; observed $($snapshotRecords.Count)."
}
$profileRecords = [regex]::Matches(
    $log,
    "event_code=profile_resolved .*config_revision=$activeConfigRevision .*" +
    "profile_id=[0-9a-f-]+ .*decision=[a-z_]+ .*" +
    "menu_slice_count=$Slices .*menu_label_count=$Slices " +
    ".*menu_source=" + $(if ($Configuration -eq "debug") { "debug_override" } else { "configuration" })
)
if ($profileRecords.Count -lt $expectedInvocations) {
    throw "Expected $expectedInvocations profile resolutions; observed $($profileRecords.Count)."
}
$invocationRecords = [regex]::Matches(
    $log,
    "event_code=invocation_started .*geometry=$expectedBehavior .*slice_count=$Slices"
)
if ($invocationRecords.Count -lt $expectedInvocations) {
    throw "Expected $expectedInvocations '$expectedBehavior' invocations; observed $($invocationRecords.Count)."
}
$menuFrameRecords = [regex]::Matches(
    $log,
    "event_code=profile_menu_frame_ready .*config_revision=$activeConfigRevision .*" +
    "slice_count=$Slices .*label_count=$Slices"
)
$minimumMenuFrames = if ($QuickFlick) { 0 } else { $expectedInvocations }
if ($menuFrameRecords.Count -lt $minimumMenuFrames) {
    throw "Expected $minimumMenuFrames complete profile-menu frames; observed $($menuFrameRecords.Count)."
}
$selectionRecords = [regex]::Matches(
    $log,
    "(?m)^(?<time>\S+) .*event_code=slice_selected .*" +
    "invocation_id=(?<invocation>[0-9a-f-]+) .*slice_index=(?<slice>[0-7]) " +
    "action_assigned=(?<assigned>true|false) plan_compiled=(?<compiled>true|false) " +
    "action_executed=false"
)
if ($selectionRecords.Count -ne $expectedInvocations) {
    throw "Expected $expectedInvocations selected gestures; observed $($selectionRecords.Count)."
}
if ($log -match "action_executed=true") {
    throw "The plan-only action milestone unexpectedly executed an action."
}
$actionOutcomeRecords = [regex]::Matches(
    $log,
    "target=actions .*event_code=(?:action_plan_compiled|invocation_cancelled) .*" +
    "invocation_id=[0-9a-f-]+ .*action_executed=false"
)
if ($actionOutcomeRecords.Count -ne $expectedInvocations) {
    throw (
        "Expected $expectedInvocations non-executing action outcomes; " +
        "observed $($actionOutcomeRecords.Count)."
    )
}
foreach ($expectedDirection in $directions) {
    $expectedSlice = $directionSlices[$expectedDirection]
    $observedForSlice = (
        $selectionRecords |
            Where-Object { [int]$_.Groups["slice"].Value -eq $expectedSlice }
    ).Count
    if ($observedForSlice -ne $InvocationCount) {
        throw (
            "Direction $expectedDirection expected slice $expectedSlice " +
            "$InvocationCount time(s); observed $observedForSlice."
        )
    }
}
if ($log -match "event_code=focus_checkpoint .*accepted=false") {
    throw "At least one live focus checkpoint failed."
}
$activationFocusCount = (
    [regex]::Matches(
        $log,
        "event_code=focus_checkpoint .*phase=activation .*accepted=true"
    )
).Count
$closedFocusCount = (
    [regex]::Matches(
        $log,
        "event_code=focus_checkpoint .*phase=after_close .*accepted=true"
    )
).Count
if (
    $activationFocusCount -lt $expectedInvocations -or
    $closedFocusCount -lt $expectedInvocations
) {
    throw (
        "Focus coverage was incomplete: activation=$activationFocusCount, " +
        "after_close=$closedFocusCount, expected=$expectedInvocations."
    )
}
if (
    $log -notmatch (
        "event_code=close_animation_completed .*duration_ms=50 forced=false"
    )
) {
    throw "The live check did not complete the bounded close animation."
}
$selectionRecord = $selectionRecords[$selectionRecords.Count - 1]
$invocationId = $selectionRecord.Groups["invocation"].Value
$closeRecord = [regex]::Match(
    $log,
    "(?m)^(?<time>\S+) .*event_code=close_animation_completed .*" +
    "invocation_id=$invocationId .*duration_ms=50 forced=false"
)
if (-not $closeRecord.Success) {
    throw "The selected invocation did not reach its normal animated close."
}
$selectionTime = [DateTimeOffset]::Parse(
    $selectionRecord.Groups["time"].Value,
    [Globalization.CultureInfo]::InvariantCulture
)
$closeTime = [DateTimeOffset]::Parse(
    $closeRecord.Groups["time"].Value,
    [Globalization.CultureInfo]::InvariantCulture
)
$observedCloseMs = ($closeTime - $selectionTime).TotalMilliseconds
if ($observedCloseMs -lt 45 -or $observedCloseMs -gt 250) {
    throw "Observed close lifecycle was $observedCloseMs ms; expected 50 ms with scheduler tolerance."
}

Write-Host (
    "Live $Configuration $Geometry $Slices-slice smoke check passed: " +
    "$expectedInvocations invocation(s), point $resolvedActivationPoint, " +
    "monitor $MonitorIndex, reduced motion $expectedReducedMotion, " +
    "maximum retained-cursor movement $maxCursorDistance px, " +
    "observed close $observedCloseMs ms."
)
