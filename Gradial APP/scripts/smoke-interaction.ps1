param(
    [ValidateSet("auto", "fixed-origin", "edge-assist")]
    [string]$Geometry = "auto",

    [ValidateSet("4", "8")]
    [string]$Slices = "8",

    [ValidateSet("debug", "release")]
    [string]$Configuration = "debug",

    [switch]$UseConfiguredMenu,

    [switch]$CaptureTiming,

    [switch]$TimingMarkersOnly,

    [ValidateRange(1, 600)]
    [int]$CaptureDurationSeconds = 4,

    [ValidateRange(0, 600)]
    [int]$WarmupSeconds = 0,

    [ValidateRange(0, 3600)]
    [int]$PostInvocationIdleSeconds = 0,

    [string]$ProcessIdFile,

    [string]$PhaseFile,

    [string]$ProgressFile,

    [string]$LogEvidenceDirectory,

    [switch]$SummaryValidationOnly,

    [string]$ForegroundProcessFile,

    [string]$TimingOutputDirectory,

    [string]$PresentMonPath,

    [ValidateRange(0, 15)]
    [int]$MonitorIndex = 0,

    [switch]$SimulateDisplayChange,

    [switch]$SimulateInputReregistration,

    [ValidateSet("none", "heartbeat", "queue-overflow")]
    [string]$SimulateInputFault = "none",

    [switch]$ReducedMotion,

    [switch]$ContinuousMeter,

    [switch]$PluginContinuous,

    [ValidateRange(0, 100)]
    [int]$ContinuousPercent = 75,

    [ValidateRange(160, 400)]
    [int]$ContinuousFullScaleDip = 300,

    [switch]$ReturnInsideBeforeRelease,

    [switch]$QuickFlick,

    [switch]$ExpectDelayExecution,

    [switch]$ExpectShortcutExecution,

    [switch]$ExpectTextExecution,

    [switch]$ExpectOpenExecution,

    [switch]$ExpectProcessExecution,

    [switch]$ExpectWindowExecution,

    [switch]$ExpectActionFailure,

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

    [string]$ChangeFromDirection = "",

    [ValidateRange(0, 2000)]
    [int]$PointerBurstEvents = 0,

    [ValidateRange(0, 1000)]
    [int]$PointerBurstSettleMilliseconds = 100,

    [switch]$RequireOpenAnimationComplete,

    [switch]$AllDirections,

    [ValidateRange(1, 10000)]
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
    private static extern int GetSystemMetrics(int index);

    [DllImport("user32.dll")]
    public static extern bool GetCursorPos(out Point point);

    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromPoint(Point point, uint flags);

    [DllImport("shcore.dll")]
    private static extern int GetDpiForMonitor(
        IntPtr monitor,
        int dpiType,
        out uint dpiX,
        out uint dpiY
    );

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

    public static void SendM4(bool down)
    {
        SendMouse(0, 0, 1, down ? 0x0080u : 0x0100u);
    }

    public static void SendRelativeMove(int dx, int dy)
    {
        SendMouse(dx, dy, 0, 0x0001u);
    }

    public static void SendAbsoluteJitterBurst(int targetX, int targetY, int eventCount)
    {
        if (eventCount <= 0 || eventCount % 2 != 0) {
            throw new ArgumentOutOfRangeException(
                "eventCount",
                "The pointer burst must contain a positive even number of events."
            );
        }
        var virtualLeft = GetSystemMetrics(76);
        var virtualTop = GetSystemMetrics(77);
        var virtualWidth = GetSystemMetrics(78);
        var virtualHeight = GetSystemMetrics(79);
        if (virtualWidth <= 1 || virtualHeight <= 1) {
            throw new InvalidOperationException("Windows reported an invalid virtual desktop.");
        }
        var inputs = new Input[eventCount];
        for (var index = 0; index < eventCount; index++) {
            var x = targetX + (index % 2 == 0 ? 1 : 0);
            var normalizedX = (int)Math.Round(
                (x - virtualLeft) * 65535.0 / (virtualWidth - 1)
            );
            var normalizedY = (int)Math.Round(
                (targetY - virtualTop) * 65535.0 / (virtualHeight - 1)
            );
            inputs[index] = new Input {
                Type = 0,
                Mouse = new MouseInput {
                    Dx = normalizedX,
                    Dy = normalizedY,
                    MouseData = 0,
                    Flags = 0xE001u,
                    Time = 0,
                    ExtraInfo = UIntPtr.Zero
                }
            };
        }
        if (SendInput((uint)eventCount, inputs, Marshal.SizeOf(typeof(Input))) != eventCount) {
            throw new InvalidOperationException("SendInput did not submit the pointer burst.");
        }
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
if ($ContinuousMeter -and $Configuration -ne "debug") {
    throw "The inert continuous meter is available only in a debug build."
}
if ($ContinuousMeter -and $PluginContinuous) {
    throw "Choose either the inert continuous meter or the configured plugin route."
}
$continuousGesture = $ContinuousMeter -or $PluginContinuous
$expectedActionModes = @(
    $ExpectDelayExecution,
    $ExpectShortcutExecution,
    $ExpectTextExecution,
    $ExpectOpenExecution,
    $ExpectProcessExecution,
    $ExpectWindowExecution,
    $ExpectActionFailure
) | Where-Object { $_ }
if ($expectedActionModes.Count -gt 1) {
    throw "Choose only one expected action execution mode."
}
if ($continuousGesture -and $expectedActionModes.Count -ne 0) {
    throw "A continuous gesture cannot also execute a discrete action fixture."
}
if (
    $expectedActionModes.Count -eq 1 -and
    (($Configuration -ne "release" -and -not $UseConfiguredMenu) -or $Slices -ne "8")
) {
    throw "Action execution validation requires a configured eight-slice menu."
}
if ($Slices -eq "4" -and -not $AllDirections -and $Direction.Length -ne 1) {
    throw "Four-slice validation accepts only N, E, S, or W."
}
if (-not [string]::IsNullOrWhiteSpace($ChangeFromDirection)) {
    $allowedChangeDirections = if ($Slices -eq "4") {
        @("N", "E", "S", "W")
    } else {
        @("N", "NE", "E", "SE", "S", "SW", "W", "NW")
    }
    if ($ChangeFromDirection -notin $allowedChangeDirections) {
        throw "ChangeFromDirection is not valid for the selected slice count."
    }
    if (-not $continuousGesture -or $AllDirections) {
        throw "ChangeFromDirection requires one continuous direction."
    }
}
if ($PointerBurstEvents -ne 0) {
    if ($PointerBurstEvents % 2 -ne 0) {
        throw "PointerBurstEvents must be even so the cursor finishes at the requested point."
    }
    if (
        -not $continuousGesture -or
        $AllDirections -or
        ($InvocationCount -ne 1 -and -not $SummaryValidationOnly)
    ) {
        throw "PointerBurstEvents requires a single direction; repeated bursts are reserved for summary-only soak validation."
    }
}
if ($ReturnInsideBeforeRelease -and (-not $continuousGesture -or $AllDirections)) {
    throw "ReturnInsideBeforeRelease requires one continuous direction."
}
if ($RequireOpenAnimationComplete -and ($QuickFlick -or $GestureDelayMilliseconds -lt 60)) {
    throw "RequireOpenAnimationComplete requires a gesture delay of at least 60 ms."
}
if (
    $SummaryValidationOnly -and
    (-not $PluginContinuous -or -not $LogEvidenceDirectory -or $AllDirections)
) {
    throw "SummaryValidationOnly requires PluginContinuous, LogEvidenceDirectory, and one direction."
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
$gradialExitCode = $null
$timingCapture = $null
$archivedLogHashes = @{}
$logEvidenceSequence = 0

function Set-PerformancePhase {
    param([string]$Name)

    if ($PhaseFile) {
        Set-Content -LiteralPath $PhaseFile -Value $Name -Encoding utf8
    }
}

function Set-InvocationProgress {
    param([int]$Completed)

    if (
        $ProgressFile -and
        ($Completed -eq 0 -or $Completed % 25 -eq 0 -or $Completed -eq $InvocationCount)
    ) {
        Set-Content -LiteralPath $ProgressFile -Value $Completed -Encoding ascii
    }
}

function Save-LogEvidence {
    param(
        [string]$Label,
        [bool]$IncludeCurrent
    )

    if (-not $LogEvidenceDirectory) {
        return
    }
    New-Item -ItemType Directory -Path $LogEvidenceDirectory -Force | Out-Null
    $sourceDirectory = Join-Path $env:LOCALAPPDATA "Gradial\logs"
    if (-not (Test-Path -LiteralPath $sourceDirectory)) {
        return
    }
    $parts = Get-ChildItem -LiteralPath $sourceDirectory -Filter "gradial*.log" |
        Sort-Object LastWriteTime, Name
    foreach ($part in $parts) {
        if (-not $IncludeCurrent -and $part.Name -match "CURRENT") {
            continue
        }
        $source = [IO.File]::Open(
            $part.FullName,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
        )
        try {
            $memory = [IO.MemoryStream]::new()
            try {
                $source.CopyTo($memory)
                $bytes = $memory.ToArray()
            }
            finally {
                $memory.Dispose()
            }
        }
        finally {
            $source.Dispose()
        }
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try {
            $hash = ([BitConverter]::ToString($sha256.ComputeHash($bytes))).Replace(
                "-",
                ""
            ).ToLowerInvariant()
        }
        finally {
            $sha256.Dispose()
        }
        if ($script:archivedLogHashes.ContainsKey($hash)) {
            continue
        }
        $script:archivedLogHashes[$hash] = $true
        $script:logEvidenceSequence++
        $destination = Join-Path $LogEvidenceDirectory (
            "{0:D6}-{1}-{2}-{3}" -f $script:logEvidenceSequence, $Label, $hash.Substring(0, 16), $part.Name
        )
        [IO.File]::WriteAllBytes($destination, $bytes)
    }
}

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
    $effectiveDpi = [GradialSmokeNative]::EffectiveDpiAt($testX, $testY)
    $gestureDistanceDip = if ($continuousGesture) {
        107.0 + ($ContinuousFullScaleDip - 107.0) * ($ContinuousPercent / 100.0)
    } else {
        80.0
    }
    $gestureDistancePx = $gestureDistanceDip * $effectiveDpi / 96.0
    $diagonalDistancePx = $gestureDistancePx / [Math]::Sqrt(2.0)
    # A one-pixel cardinal bias keeps the 0% target on the armed side of the
    # inclusive threshold after DPI conversion and integer cursor quantization.
    # Diagonals use nearest rounding to minimize scalar error across the
    # configured 107-to-full-scale range.
    $continuousBoundaryBias = if ($continuousGesture -and $ContinuousPercent -eq 0) {
        1
    } else {
        0
    }
    $cardinal = if ($continuousGesture) {
        [int][Math]::Ceiling($gestureDistancePx) + $continuousBoundaryBias
    } else {
        [int][Math]::Round($gestureDistancePx)
    }
    $diagonal = if ($continuousGesture) {
        [int][Math]::Round($diagonalDistancePx)
    } else {
        [int][Math]::Round($diagonalDistancePx)
    }
    $directionVectors = @{
        N  = @(0, -$cardinal)
        NE = @($diagonal, -$diagonal)
        E  = @($cardinal, 0)
        SE = @($diagonal, $diagonal)
        S  = @(0, $cardinal)
        SW = @(-$diagonal, $diagonal)
        W  = @(-$cardinal, 0)
        NW = @(-$diagonal, -$diagonal)
    }
    $insideDistancePx = 84.0 * $effectiveDpi / 96.0
    $insideCardinal = [int][Math]::Round($insideDistancePx)
    $insideDiagonal = [int][Math]::Round($insideDistancePx / [Math]::Sqrt(2.0))
    $insideDirectionVectors = @{
        N  = @(0, -$insideCardinal)
        NE = @($insideDiagonal, -$insideDiagonal)
        E  = @($insideCardinal, 0)
        SE = @($insideDiagonal, $insideDiagonal)
        S  = @(0, $insideCardinal)
        SW = @(-$insideDiagonal, $insideDiagonal)
        W  = @(-$insideCardinal, 0)
        NW = @(-$insideDiagonal, -$insideDiagonal)
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
        if (-not $PluginContinuous -and -not $UseConfiguredMenu) {
            $startupArguments += "--slices=$Slices"
        }
        $startupArguments += "--skip-onboarding"
        if ($ContinuousMeter) {
            $startupArguments += "--continuous-meter"
            $startupArguments += "--continuous-full-scale-dip=$ContinuousFullScaleDip"
        }
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
    if ($ProcessIdFile) {
        Set-Content -LiteralPath $ProcessIdFile -Value $gradial.Id -Encoding ascii
    }
    Set-PerformancePhase "startup"
    $logPath = Join-Path $env:LOCALAPPDATA "Gradial\logs\gradial_rCURRENT.log"
    $startupLog = Get-Content -Raw -LiteralPath $logPath
    $startupConfigurationRecord = [regex]::Match(
        $startupLog,
        "event_code=config_loaded .*config_revision=(?<revision>[1-9][0-9]*) " +
        "schema_version=6 source=(?<source>[a-z_]+) " +
        "migrated_from_schema=(?<migration>none|1|2|3|4|5) action_count=(?<actions>[0-9]+)"
    )
    if (-not $startupConfigurationRecord.Success) {
        $observedConfiguration = [regex]::Match(
            $startupLog,
            "(?m)^.*event_code=config_loaded.*$"
        ).Value.Trim()
        if (-not $observedConfiguration) {
            $observedConfiguration = "no config_loaded event"
        }
        throw (
            "The live check failed startup configuration preflight. " +
            "Observed: $observedConfiguration"
        )
    }
    Set-InvocationProgress 0
    Save-LogEvidence "startup" $true
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
    Set-PerformancePhase "warmup"
    if ($WarmupSeconds -gt 0) {
        Start-Sleep -Seconds $WarmupSeconds
    }
    Set-PerformancePhase "invocations"
    if ($CaptureTiming) {
        $captureScript = Join-Path $PSScriptRoot "capture-timing.ps1"
        $captureArguments = "-NoProfile -ExecutionPolicy Bypass -File `"$captureScript`" " +
            "-DurationSeconds $CaptureDurationSeconds -ProcessId $($gradial.Id)"
        if ($TimingMarkersOnly) {
            $captureArguments += " -MarkersOnly"
        }
        if ($TimingOutputDirectory) {
            $captureArguments += " -OutputDirectory `"$TimingOutputDirectory`""
        }
        if ($PresentMonPath) {
            $captureArguments += " -PresentMonPath `"$PresentMonPath`""
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
    if ($ForegroundProcessFile) {
        Set-Content `
            -LiteralPath $ForegroundProcessFile `
            -Value $foregroundProcess `
            -Encoding ascii
    }
    $gestureDelay = if ($QuickFlick) { 0 } else { $GestureDelayMilliseconds }
    $continuousUsesFixedOrigin = (
        $continuousGesture -and
        (
            $Geometry -eq "fixed-origin" -or
            ($Geometry -eq "auto" -and $resolvedActivationPoint -eq "center")
        )
    )
    if ($PointerBurstEvents -ne 0 -and -not $continuousUsesFixedOrigin) {
        throw "PointerBurstEvents validates only the fixed-origin continuous path."
    }
    $maxCursorDistance = 0.0
    $completedInvocations = 0
    foreach ($gestureDirection in $directions) {
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
            $gesturePath = if ([string]::IsNullOrWhiteSpace($ChangeFromDirection)) {
                @($gestureDirection)
            } else {
                @($ChangeFromDirection, $gestureDirection)
            }
            $currentOffsetX = 0
            $currentOffsetY = 0
            foreach ($pathDirection in $gesturePath) {
                $targetVector = $directionVectors[$pathDirection]
                [GradialSmokeNative]::SendRelativeMove(
                    $targetVector[0] - $currentOffsetX,
                    $targetVector[1] - $currentOffsetY
                )
                $currentOffsetX = $targetVector[0]
                $currentOffsetY = $targetVector[1]
                if ($continuousUsesFixedOrigin -and -not $SummaryValidationOnly) {
                    $desiredX = $testX + $currentOffsetX
                    $desiredY = $testY + $currentOffsetY
                    $correctionTolerance = if ($continuousGesture) { 0 } else { 1 }
                    for ($correction = 0; $correction -lt 8; $correction++) {
                        Start-Sleep -Milliseconds 10
                        $correctedCursor = New-Object GradialSmokeNative+Point
                        [GradialSmokeNative]::GetCursorPos([ref]$correctedCursor) | Out-Null
                        $errorX = $desiredX - $correctedCursor.X
                        $errorY = $desiredY - $correctedCursor.Y
                        if (
                            [Math]::Abs($errorX) -le $correctionTolerance -and
                            [Math]::Abs($errorY) -le $correctionTolerance
                        ) {
                            break
                        }
                        [GradialSmokeNative]::SendRelativeMove($errorX, $errorY)
                    }
                }
                if ($gestureDelay -gt 0) {
                    Start-Sleep -Milliseconds $gestureDelay
                }
            }
            if ($ReturnInsideBeforeRelease) {
                $insideVector = $insideDirectionVectors[$gestureDirection]
                [GradialSmokeNative]::SendRelativeMove(
                    $insideVector[0] - $currentOffsetX,
                    $insideVector[1] - $currentOffsetY
                )
                $currentOffsetX = $insideVector[0]
                $currentOffsetY = $insideVector[1]
                if ($continuousUsesFixedOrigin -and -not $SummaryValidationOnly) {
                    $desiredX = $testX + $currentOffsetX
                    $desiredY = $testY + $currentOffsetY
                    for ($correction = 0; $correction -lt 8; $correction++) {
                        Start-Sleep -Milliseconds 10
                        $correctedCursor = New-Object GradialSmokeNative+Point
                        [GradialSmokeNative]::GetCursorPos([ref]$correctedCursor) | Out-Null
                        $errorX = $desiredX - $correctedCursor.X
                        $errorY = $desiredY - $correctedCursor.Y
                        if ($errorX -eq 0 -and $errorY -eq 0) {
                            break
                        }
                        [GradialSmokeNative]::SendRelativeMove($errorX, $errorY)
                    }
                }
                if ($gestureDelay -gt 0) {
                    Start-Sleep -Milliseconds $gestureDelay
                }
            }
            if ($PointerBurstEvents -ne 0) {
                [GradialSmokeNative]::SendAbsoluteJitterBurst(
                    $testX + $currentOffsetX,
                    $testY + $currentOffsetY,
                    $PointerBurstEvents
                )
                if ($PointerBurstSettleMilliseconds -gt 0) {
                    Start-Sleep -Milliseconds $PointerBurstSettleMilliseconds
                }
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
            Set-InvocationProgress $completedInvocations
            if ($SummaryValidationOnly -and $completedInvocations % 100 -eq 0) {
                Save-LogEvidence "rotation" $false
            }
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
    if (
        -not $ExpectWindowExecution -and
        $foregroundAfter -ne $foregroundWindow -and
        $foregroundProcessAfter -ne $foregroundProcess
    ) {
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
    Set-PerformancePhase "idle"
    if ($PostInvocationIdleSeconds -gt 0) {
        Start-Sleep -Seconds $PostInvocationIdleSeconds
    }
    Set-PerformancePhase "complete"
}
finally {
    if ($m4IsDown) {
        [GradialSmokeNative]::SendM4($false)
    }
    [GradialSmokeNative]::SetCursorPos($originalCursor.X, $originalCursor.Y) | Out-Null
    if ($gradial -and -not $gradial.HasExited) {
        $gracefulShutdownRequested =
            [GradialSmokeNative]::CloseControllerWindowForProcess([uint32]$gradial.Id)
        if (-not $gracefulShutdownRequested) {
            Stop-Process -Id $gradial.Id -Force
            throw "Could not request a graceful Gradial shutdown."
        }
        if (-not $gradial.WaitForExit(5000)) {
            Stop-Process -Id $gradial.Id -Force
        }
        $gradial.Refresh()
        if ($gradial.HasExited) {
            $gradialExitCode = $gradial.ExitCode
        }
    }
    Save-LogEvidence "shutdown" $true
    if ($timingCapture -and -not $timingCapture.HasExited) {
        if (-not $timingCapture.WaitForExit(($CaptureDurationSeconds + 5) * 1000)) {
            Stop-Process -Id $timingCapture.Id -Force
        }
    }
}

$logPath = Join-Path $env:LOCALAPPDATA "Gradial\logs\gradial_rCURRENT.log"
$log = Get-Content -LiteralPath $logPath -Raw
if ($SummaryValidationOnly) {
    $expectedInvocations = $directions.Count * $InvocationCount
    $interactionStop = [regex]::Match(
        $log,
        "event_code=interaction_thread_stopped .*" +
        "activation_down_events=(?<down>[0-9]+) .*" +
        "activation_up_events=(?<up>[0-9]+) .*" +
        "continuous_updates_dispatched=(?<dispatched>[0-9]+) " +
        "continuous_updates_prioritized=(?<prioritized>[0-9]+) " +
        "continuous_updates_coalesced=(?<coalesced>[0-9]+) cancel_events=(?<cancel>[0-9]+)"
    )
    if (
        -not $interactionStop.Success -or
        [int]$interactionStop.Groups["down"].Value -ne $expectedInvocations -or
        [int]$interactionStop.Groups["up"].Value -ne $expectedInvocations -or
        [int]$interactionStop.Groups["cancel"].Value -ne 0 -or
        [int]$interactionStop.Groups["dispatched"].Value -eq 0 -or
        [int]$interactionStop.Groups["prioritized"].Value -eq 0 -or
        ($PointerBurstEvents -gt 0 -and [int]$interactionStop.Groups["coalesced"].Value -eq 0)
    ) {
        throw "The summary-only interaction counters did not satisfy the soak contract."
    }
    if ($gradialExitCode -ne 0) {
        throw "Gradial did not exit cleanly; exit code was $gradialExitCode."
    }
    $stageCounts = @{}
    foreach ($stage in @("begin", "update", "commit", "cancel")) {
        $record = [regex]::Match(
            $log,
            "event_code=plugin_callback_timing result_code=ok stage=$stage " +
            "callback_count=(?<count>[0-9]+) .*histogram_bucket_count=(?<buckets>[0-9]+) " +
            "result_ok_count=(?<ok>[0-9]+) result_failure_count=(?<failed>[0-9]+) " +
            "active_session_at_shutdown=(?<active>true|false)"
        )
        if (
            -not $record.Success -or
            [int]$record.Groups["buckets"].Value -ne 15 -or
            [int]$record.Groups["failed"].Value -ne 0 -or
            $record.Groups["active"].Value -ne "false"
        ) {
            throw "The summary-only plugin timing record for $stage was incomplete."
        }
        $stageCounts[$stage] = [int]$record.Groups["ok"].Value
    }
    if (
        $stageCounts.begin -lt $expectedInvocations -or
        ($stageCounts.commit + $stageCounts.cancel) -ne $stageCounts.begin
    ) {
        throw (
            "Plugin lifecycle totals were not exact: begin=$($stageCounts.begin), " +
            "commit=$($stageCounts.commit), cancel=$($stageCounts.cancel)."
        )
    }
    foreach ($eventCode in @(
        "action_executor_stopped",
        "interaction_thread_stopped",
        "hook_thread_stopped",
        "plugin_worker_stopped"
    )) {
        if ($log -notmatch "event_code=$eventCode .*result_code=ok") {
            throw "The summary-only run did not retain $eventCode."
        }
    }
    Write-Host (
        "Live $Configuration plugin continuous summary smoke check passed: " +
        "$expectedInvocations invocation(s), $($stageCounts.update) Update callback(s), " +
        "$($interactionStop.Groups['coalesced'].Value) coalesced visual update(s), exit code 0."
    )
    return
}
$configurationRecord = [regex]::Match(
    $log,
    "event_code=config_loaded .*config_revision=(?<revision>[1-9][0-9]*) " +
    "schema_version=6 source=(?<source>[a-z_]+) " +
    "migrated_from_schema=(?<migration>none|1|2|3|4|5) action_count=(?<actions>[0-9]+)"
)
if (-not $configurationRecord.Success) {
    throw "The live check did not load a validated schema-six configuration."
}
$activeConfigRevision = $configurationRecord.Groups["revision"].Value
if (
    $log -notmatch (
        "event_code=action_executor_ready .*config_revision=$activeConfigRevision " +
        ".*supported_operations=delay,send_shortcut,type_text,open_url,open_path,run_executable,window " +
        ".*per_step_timeout_ms=5000 " +
        ".*maximum_duration_ms=30000 .*maximum_steps=32 " +
        ".*cancellation_poll_ms=10 .*shutdown_wait_ms=2000"
    )
) {
    throw "The bounded action executor did not initialize with the required safety limits."
}
$expectedDebugOverride = if (
    $Configuration -eq "debug" -and -not $PluginContinuous -and -not $UseConfiguredMenu
) {
    $Slices
} else {
    "none"
}
$expectedContinuousMeter = if ($ContinuousMeter) { "true" } else { "false" }
$expectedFullScaleDip = if ($ContinuousMeter) { $ContinuousFullScaleDip.ToString() } else { "300" }
$expectedOverlayHalfExtent = if ($ContinuousMeter) {
    ($ContinuousFullScaleDip + 20).ToString()
} else {
    "144"
}
if (
    $log -notmatch (
        "event_code=input_geometry_ready .*geometry_mode=$Geometry " +
        ".*debug_slice_override=$expectedDebugOverride " +
        ".*debug_continuous_meter=$expectedContinuousMeter " +
        ".*overlay_half_extent_dip=$expectedOverlayHalfExtent " +
        ".*continuous_full_scale_radius_dip=$expectedFullScaleDip"
    )
) {
    throw "The log does not confirm geometry mode '$Geometry' and the expected menu authority."
}
$expectedReducedMotion = if ($ReducedMotion) { "true" } else { "false" }
$expectedOverlaySize = if ($ContinuousMeter) {
    (($ContinuousFullScaleDip + 20) * 2).ToString()
} else {
    "400"
}
if (
    $log -notmatch (
        "event_code=renderer_geometry_cache_ready .*slice_counts=4,8 " +
        ".*reduced_motion=$expectedReducedMotion .*open_ms=60 " +
        ".*selection_ms=0 .*close_ms=50 " +
        ".*continuous_arm_radius_dip=107 .*overlay_size_dip=$expectedOverlaySize"
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
    ".*menu_source=" + $(if (
        $Configuration -eq "debug" -and -not $PluginContinuous -and -not $UseConfiguredMenu
    ) {
        "debug_override"
    } else {
        "configuration"
    })
)
if ($profileRecords.Count -lt $expectedInvocations) {
    throw "Expected $expectedInvocations profile resolutions; observed $($profileRecords.Count)."
}
$invocationRecords = [regex]::Matches(
    $log,
    "(?m)^(?<time>\S+) .*event_code=invocation_started .*" +
    "invocation_id=(?<invocation>[0-9a-f-]+) .*" +
    "geometry=$expectedBehavior .*slice_count=$Slices"
)
if ($invocationRecords.Count -lt $expectedInvocations) {
    throw "Expected $expectedInvocations '$expectedBehavior' invocations; observed $($invocationRecords.Count)."
}
$menuFrameRecords = [regex]::Matches(
    $log,
    "(?m)^(?<time>\S+) .*event_code=profile_menu_frame_ready .*" +
    "config_revision=$activeConfigRevision .*" +
    "invocation_id=(?<invocation>[0-9a-f-]+) .*" +
    "slice_count=$Slices .*label_count=$Slices"
)
$minimumMenuFrames = if ($QuickFlick) { 0 } else { $expectedInvocations }
if ($menuFrameRecords.Count -lt $minimumMenuFrames) {
    throw "Expected $minimumMenuFrames complete profile-menu frames; observed $($menuFrameRecords.Count)."
}
$firstFrameTimes = @{}
foreach ($record in $menuFrameRecords) {
    $id = $record.Groups["invocation"].Value
    if (-not $firstFrameTimes.ContainsKey($id)) {
        $firstFrameTimes[$id] = [DateTimeOffset]::Parse(
            $record.Groups["time"].Value,
            [Globalization.CultureInfo]::InvariantCulture
        )
    }
}
$maximumFirstFrameMs = 0.0
if (-not $QuickFlick) {
    foreach ($record in $invocationRecords) {
        $id = $record.Groups["invocation"].Value
        if (-not $firstFrameTimes.ContainsKey($id)) {
            throw "Invocation $id did not render a first menu frame."
        }
        $startedAt = [DateTimeOffset]::Parse(
            $record.Groups["time"].Value,
            [Globalization.CultureInfo]::InvariantCulture
        )
        $firstFrameMs = ($firstFrameTimes[$id] - $startedAt).TotalMilliseconds
        $maximumFirstFrameMs = [Math]::Max($maximumFirstFrameMs, $firstFrameMs)
        if ($firstFrameMs -lt 0 -or $firstFrameMs -gt 100) {
            throw "Invocation $id rendered its first frame after $firstFrameMs ms; the limit is 100 ms."
        }
    }
}
$openCompletionTimes = @{}
if ($RequireOpenAnimationComplete) {
    $openCompletionRecords = [regex]::Matches(
        $log,
        "(?m)^(?<time>\S+) .*event_code=open_animation_completed .*" +
        "config_revision=$activeConfigRevision .*" +
        "invocation_id=(?<invocation>[0-9a-f-]+) .*duration_ms=60"
    )
    foreach ($record in $openCompletionRecords) {
        $id = $record.Groups["invocation"].Value
        if (-not $openCompletionTimes.ContainsKey($id)) {
            $openCompletionTimes[$id] = [DateTimeOffset]::Parse(
                $record.Groups["time"].Value,
                [Globalization.CultureInfo]::InvariantCulture
            )
        }
    }
}
$selectionRecords = if ($continuousGesture -and -not $ReturnInsideBeforeRelease) {
    [regex]::Matches(
        $log,
        "(?m)^(?<time>\S+) .*event_code=continuous_meter_committed .*" +
        "invocation_id=(?<invocation>[0-9a-f-]+) .*slice_index=(?<slice>[0-7]) " +
        "normalized_value=(?<value>[0-9]+\.[0-9]+) " +
        "continuous_meter=$($ContinuousMeter.ToString().ToLowerInvariant()) action_executed=false"
    )
} else {
    [regex]::Matches(
        $log,
        "(?m)^(?<time>\S+) .*event_code=slice_selected .*" +
        "invocation_id=(?<invocation>[0-9a-f-]+) .*slice_index=(?<slice>[0-7]) " +
        "action_assigned=(?<assigned>true|false) plan_compiled=(?<compiled>true|false) " +
        "action_executed=false"
    )
}
if ($selectionRecords.Count -ne $expectedInvocations) {
    $outcomeName = if ($continuousGesture -and -not $ReturnInsideBeforeRelease) {
        "continuous commits"
    } else {
        "selected gestures"
    }
    throw "Expected $expectedInvocations $outcomeName; observed $($selectionRecords.Count)."
}
if (-not $QuickFlick) {
    foreach ($record in $selectionRecords) {
        $id = $record.Groups["invocation"].Value
        $terminalTime = [DateTimeOffset]::Parse(
            $record.Groups["time"].Value,
            [Globalization.CultureInfo]::InvariantCulture
        )
        if (-not $firstFrameTimes.ContainsKey($id) -or $firstFrameTimes[$id] -ge $terminalTime) {
            throw "Invocation $id did not render before its release outcome."
        }
    }
}
if ($RequireOpenAnimationComplete) {
    foreach ($record in $selectionRecords) {
        $id = $record.Groups["invocation"].Value
        $terminalTime = [DateTimeOffset]::Parse(
            $record.Groups["time"].Value,
            [Globalization.CultureInfo]::InvariantCulture
        )
        if (
            -not $openCompletionTimes.ContainsKey($id) -or
            $openCompletionTimes[$id] -ge $terminalTime
        ) {
            throw "Invocation $id did not finish its opening animation before release."
        }
    }
}
if ($continuousGesture -and -not $ReturnInsideBeforeRelease) {
    $unexpectedDiscreteSelections = [regex]::Matches($log, "event_code=slice_selected")
    if ($unexpectedDiscreteSelections.Count -ne 0) {
        throw "The continuous meter unexpectedly produced a discrete slice selection."
    }
    $expectedValue = $ContinuousPercent / 100.0
    foreach ($record in $selectionRecords) {
        $observedValue = [double]::Parse(
            $record.Groups["value"].Value,
            [Globalization.CultureInfo]::InvariantCulture
        )
        if ([Math]::Abs($observedValue - $expectedValue) -gt 0.02) {
            throw (
                "Continuous target $expectedValue produced $observedValue; " +
                "the tolerance is 0.02."
            )
        }
    }
}
if ($ReturnInsideBeforeRelease) {
    $unexpectedContinuousCommits = [regex]::Matches($log, "event_code=continuous_meter_committed")
    if ($unexpectedContinuousCommits.Count -ne 0) {
        throw "Returning inside the wedge unexpectedly committed a continuous value."
    }
    $armedUpdates = [regex]::Matches(
        $log,
        "event_code=continuous_meter_updated .*armed=true action_executed=false"
    )
    $discreteUpdates = [regex]::Matches(
        $log,
        "event_code=continuous_meter_updated .*armed=false action_executed=false"
    )
    if (
        $armedUpdates.Count -lt $expectedInvocations -or
        $discreteUpdates.Count -lt $expectedInvocations
    ) {
        throw "The live meter did not report both outside and restored discrete states."
    }
    if ($log -match "event_code=action_execution_started") {
        throw "The inert hybrid meter unexpectedly started a configured action."
    }
}
if ($expectedActionModes.Count -eq 1) {
    $uncompiledSelections = @(
        $selectionRecords |
            Where-Object {
                $_.Groups["assigned"].Value -ne "true" -or
                $_.Groups["compiled"].Value -ne "true"
            }
    )
    if ($uncompiledSelections.Count -ne 0) {
        throw "At least one action fixture selection did not compile its assigned action."
    }
    if ($ExpectActionFailure) {
        $failedActionRecords = [regex]::Matches(
            $log,
            "target=actions .*event_code=action_execution_completed " +
            ".*result_code=failure .*error_code=path_not_found " +
            ".*action_executed=false"
        )
        if ($failedActionRecords.Count -ne $expectedInvocations) {
            throw (
                "Expected $expectedInvocations path-not-found action failures; " +
                "observed $($failedActionRecords.Count)."
            )
        }
        $activityRecords = [regex]::Matches(
            $log,
            "target=tray .*event_code=action_activity_recorded " +
            ".*result_code=failure .*error_code=path_not_found " +
            ".*notification_queued=true"
        )
        if ($activityRecords.Count -ne $expectedInvocations) {
            throw (
                "Expected $expectedInvocations retained failure records; " +
                "observed $($activityRecords.Count)."
            )
        }
        $notificationRecords = [regex]::Matches(
            $log,
            "target=tray .*event_code=action_failure_notification_shown " +
            ".*result_code=ok .*error_code=path_not_found"
        )
        if ($notificationRecords.Count -ne $expectedInvocations) {
            throw (
                "Expected $expectedInvocations non-focus-stealing failure notifications; " +
                "observed $($notificationRecords.Count)."
            )
        }
        if (
            $log -match (
                "target=actions .*event_code=action_execution_completed " +
                ".*result_code=ok .*action_executed=true"
            )
        ) {
            throw "The failing action fixture unexpectedly reported success."
        }
    }
    else {
        $completedActionRecords = [regex]::Matches(
            $log,
            "target=actions .*event_code=action_execution_completed .*result_code=ok " +
            ".*action_executed=true"
        )
        if ($completedActionRecords.Count -ne $expectedInvocations) {
            throw (
                "Expected $expectedInvocations completed actions; " +
                "observed $($completedActionRecords.Count)."
            )
        }
        if (
            $log -match (
                "event_code=(?:action_activity_recorded|" +
                "action_failure_notification_shown)"
            )
        ) {
            throw "A successful action produced user-visible failure activity."
        }
    }
}
elseif ($log -match "action_executed=true") {
    throw "The action-free smoke configuration unexpectedly executed an action."
}
if ($continuousGesture) {
    if (
        $log -match (
            "target=actions .*event_code=(?:action_plan_compiled|action_dispatched) .*" +
            "action_executed="
        )
    ) {
        throw "The continuous meter reached the saved-action compilation or dispatch path."
    }

    $armedInvocationRecords = [regex]::Matches(
        $log,
        "event_code=continuous_meter_updated .*" +
        "invocation_id=(?<invocation>[0-9a-f-]+) .*armed=true action_executed=false"
    )
    $armedInvocationIds = @(
        $armedInvocationRecords |
            ForEach-Object { $_.Groups["invocation"].Value } |
            Sort-Object -Unique
    )
    foreach ($id in $armedInvocationIds) {
        if (
            $log -notmatch (
                "event_code=live_session_started result_code=ok .*" +
                "invocation_id=$([regex]::Escape($id)) .*backend=" +
                $(if ($ContinuousMeter) { "inert" } else { "plugin" })
            )
        ) {
            throw "Armed invocation $id did not start the expected continuous session."
        }
        if ($ContinuousMeter -and (
            $log -notmatch (
                "event_code=inert_backend_request_validated result_code=ok .*" +
                "invocation_id=$([regex]::Escape($id)) stage=Begin"
            )
        )) {
            throw "Armed invocation $id did not validate and deliver Begin."
        }
    }

    if (-not $ReturnInsideBeforeRelease) {
        foreach ($record in $selectionRecords) {
            $id = $record.Groups["invocation"].Value
            if ($ContinuousMeter -and (
                $log -notmatch (
                    "event_code=inert_backend_request_validated result_code=ok .*" +
                    "invocation_id=$([regex]::Escape($id)) stage=Commit"
                )
            )) {
                throw "Continuous invocation $id did not validate and deliver Commit."
            }
        }
    }
    if ($ReturnInsideBeforeRelease) {
        foreach ($record in $selectionRecords) {
            $id = $record.Groups["invocation"].Value
            if ($ContinuousMeter -and (
                $log -notmatch (
                    "event_code=inert_backend_request_validated result_code=ok .*" +
                    "invocation_id=$([regex]::Escape($id)) stage=Cancel " +
                    "cancel_reason=ReturnedToDiscrete"
                )
            )) {
                throw "Hybrid invocation $id did not suspend with returned-to-discrete."
            }
        }
    }
    if ($ChangeFromDirection -and $ChangeFromDirection -ne $Direction) {
        $directionHandoffs = [regex]::Matches(
            $log,
            "event_code=inert_backend_request_validated result_code=ok .*" +
            "stage=Cancel cancel_reason=DirectionChanged"
        )
        if ($ContinuousMeter -and $directionHandoffs.Count -lt $expectedInvocations) {
            throw "The inert backend did not observe every unlocked direction handoff."
        }
    }
    if ($PluginContinuous) {
        $pluginBegins = [regex]::Matches(
            $log,
            "event_code=plugin_callback_timing result_code=ok stage=begin " +
            "callback_count=(?<count>[0-9]+)"
        ) | Select-Object -Last 1
        $pluginCommits = [regex]::Matches(
            $log,
            "event_code=plugin_callback_timing result_code=ok stage=commit " +
            "callback_count=(?<count>[0-9]+)"
        ) | Select-Object -Last 1
        $pluginBeginCount = if ($pluginBegins) {
            [int]$pluginBegins.Groups["count"].Value
        } else {
            0
        }
        $pluginCommitCount = if ($pluginCommits) {
            [int]$pluginCommits.Groups["count"].Value
        } else {
            0
        }
        if (
            $pluginBeginCount -lt $expectedInvocations -or
            $pluginCommitCount -lt $expectedInvocations
        ) {
            throw (
                "Plugin lifecycle coverage was incomplete: " +
                "begin=$pluginBeginCount, commit=$pluginCommitCount."
            )
        }
    }
} else {
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
}
if (
    $log -notmatch (
        "event_code=action_executor_stopped .*result_code=ok " +
        "config_revision=$activeConfigRevision"
    )
) {
    throw "The action executor did not complete its bounded shutdown."
}
$interactionStop = [regex]::Match(
    $log,
    "event_code=interaction_thread_stopped .*" +
    "pointer_moved_events=(?<pointer>[0-9]+) .*" +
    "continuous_updates_dispatched=(?<dispatched>[0-9]+) " +
    "continuous_updates_prioritized=(?<prioritized>[0-9]+) " +
    "continuous_updates_coalesced=(?<coalesced>[0-9]+)"
)
if (-not $interactionStop.Success) {
    throw "The interaction shutdown record did not contain coalescing diagnostics."
}
$coalescedUpdates = [int]$interactionStop.Groups["coalesced"].Value
if ($continuousGesture -and [int]$interactionStop.Groups["prioritized"].Value -eq 0) {
    throw "The continuous gesture did not prioritize any direction or mode transition."
}
if ($PointerBurstEvents -ne 0 -and $coalescedUpdates -eq 0) {
    throw "The fixed-origin pointer burst did not coalesce any obsolete visual updates."
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
    "continuous meter $expectedContinuousMeter at $ContinuousPercent% " +
    "with $ContinuousFullScaleDip DIP full scale, " +
    "coalesced visual updates $coalescedUpdates, " +
    "maximum first-frame latency $maximumFirstFrameMs ms, " +
    "maximum retained-cursor movement $maxCursorDistance px, " +
    "observed close $observedCloseMs ms."
)
