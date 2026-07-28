param(
    [ValidateSet("auto", "fixed-origin", "edge-assist")]
    [string]$Geometry = "auto",

    [ValidateSet("debug", "release")]
    [string]$Configuration = "debug",

    [switch]$CaptureTiming,

    [switch]$TimingMarkersOnly,

    [ValidateRange(1, 30)]
    [int]$CaptureDurationSeconds = 4,

    [ValidateRange(0, 15)]
    [int]$MonitorIndex = 0,

    [switch]$SimulateDisplayChange
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
        var count = 0;
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (windowProcessId == processId && PostMessage(
                window,
                0x007Eu,
                IntPtr.Zero,
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
    if ($Geometry -eq "fixed-origin") {
        $testX = $workArea.Left + [int]($workArea.Width / 2)
        $testY = $workArea.Top + [int]($workArea.Height / 2)
    }
    else {
        $testX = $workArea.Left + 2
        $testY = $workArea.Top + 2
    }
    if (-not [GradialSmokeNative]::SetCursorPos($testX, $testY)) {
        throw "Could not position the cursor for the smoke check."
    }
    $cursorAtActivation = New-Object GradialSmokeNative+Point
    if (-not [GradialSmokeNative]::GetCursorPos([ref]$cursorAtActivation)) {
        throw "Could not read the positioned cursor."
    }

    $gradial = Start-Process -FilePath $executable `
        -ArgumentList "--geometry=$Geometry" `
        -WorkingDirectory $workspace `
        -PassThru `
        -WindowStyle Hidden
    Start-Sleep -Milliseconds 1200
    if ($gradial.HasExited) {
        throw "Gradial exited during startup with code $($gradial.ExitCode)."
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

        $logPath = Join-Path $env:LOCALAPPDATA "Gradial\logs\gradial_rCURRENT.log"
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

    [GradialSmokeNative]::SendM4($true)
    $m4IsDown = $true
    Start-Sleep -Milliseconds 180
    [GradialSmokeNative]::SendRelativeMove(80, 0)
    Start-Sleep -Milliseconds 180

    $cursorDuringGesture = New-Object GradialSmokeNative+Point
    [GradialSmokeNative]::GetCursorPos([ref]$cursorDuringGesture) | Out-Null
    [GradialSmokeNative]::SendM4($false)
    $m4IsDown = $false
    Start-Sleep -Milliseconds 250

    $foregroundAfter = [GradialSmokeNative]::GetForegroundWindow()
    $foregroundProcessAfter = [GradialSmokeNative]::ForegroundProcessId()
    if ($foregroundAfter -ne $foregroundWindow -and $foregroundProcessAfter -ne $foregroundProcess) {
        throw "The foreground application changed during the Gradial invocation."
    }
    if ($Geometry -ne "fixed-origin") {
        $cursorDistance = [Math]::Sqrt(
            [Math]::Pow($cursorDuringGesture.X - $cursorAtActivation.X, 2) +
            [Math]::Pow($cursorDuringGesture.Y - $cursorAtActivation.Y, 2)
        )
        if ($cursorDistance -gt 2) {
            throw "Edge assist moved the system cursor by $cursorDistance pixels."
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
if ($log -notmatch "event_code=input_geometry_ready .*geometry_mode=$Geometry") {
    throw "The log does not confirm geometry mode '$Geometry'."
}
if ($log -notmatch "event_code=invocation_started ") {
    throw "The live check did not produce a Gradial invocation."
}
$expectedBehavior = if ($Geometry -eq "fixed-origin") { "fixed-origin" } else { "edge-assist" }
if ($log -notmatch "event_code=invocation_started .*geometry=$expectedBehavior") {
    throw "The invocation did not resolve to expected behavior '$expectedBehavior'."
}
if ($log -notmatch "event_code=slice_selected .*slice_index=2 ") {
    throw "The live eastward gesture did not select the east slice."
}
if ($log -notmatch "event_code=focus_checkpoint .*phase=after_close .*accepted=true") {
    throw "The live check did not complete the focus checkpoints."
}

Write-Host "Live $Configuration $Geometry smoke check passed on monitor $MonitorIndex."
