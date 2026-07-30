param(
    [switch]$CaptureScreenshot
)

$ErrorActionPreference = "Stop"

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\debug\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's debug configuration before running the onboarding smoke check."
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the isolated onboarding check."
}

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class GradialOnboardingSmokeNative
{
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr data);

    [StructLayout(LayoutKind.Sequential)]
    public struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
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

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(
        IntPtr window,
        uint message,
        IntPtr wparam,
        IntPtr lparam
    );

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(
        IntPtr window,
        StringBuilder text,
        int maximum
    );

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rectangle);

    [DllImport("user32.dll")]
    private static extern uint SendInput(uint count, Input[] inputs, int size);

    [DllImport("user32.dll")]
    private static extern void keybd_event(
        byte virtualKey,
        byte scanCode,
        uint flags,
        UIntPtr extraInfo
    );

    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);

    [DllImport("shell32.dll")]
    private static extern int Shell_NotifyIconGetRect(
        ref NotificationIconIdentifier identifier,
        out Rect iconLocation
    );

    public static bool SendKey(IntPtr window, uint virtualKey)
    {
        return
            PostMessage(window, 0x0100u, new IntPtr(virtualKey), IntPtr.Zero) &&
            PostMessage(window, 0x0101u, new IntPtr(virtualKey), IntPtr.Zero);
    }

    public static IntPtr FindVisibleWindow(uint processId, string title)
    {
        var found = IntPtr.Zero;
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (windowProcessId != processId || !IsWindowVisible(window)) return true;
            var text = new StringBuilder(256);
            GetWindowText(window, text, text.Capacity);
            if (text.ToString() == title) {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static bool CloseControllerWindow(uint processId)
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

    public static void SendM5(bool down)
    {
        SendMouse(0, 0, 2, down ? 0x0080u : 0x0100u);
    }

    public static void SetControl(bool down)
    {
        keybd_event(0xA2, 0x1D, down ? 0u : 0x0002u, UIntPtr.Zero);
    }

    public static void SendRelativeMove(int dx, int dy)
    {
        SendMouse(dx, dy, 0, 0x0001u);
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

function Wait-LogPattern {
    param(
        [string]$Path,
        [string]$Pattern,
        [string]$Failure
    )

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        if (Test-Path -LiteralPath $Path) {
            $contents = Get-Content -LiteralPath $Path -Raw
            if ($contents -match $Pattern) {
                return $contents
            }
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    if (Test-Path -LiteralPath $Path) {
        Get-Content -LiteralPath $Path -Tail 30 | Write-Host
    }
    throw $Failure
}

function Save-WindowScreenshot {
    param(
        [IntPtr]$Window,
        [string]$Path
    )

    $rectangle = New-Object GradialOnboardingSmokeNative+Rect
    if (
        -not [GradialOnboardingSmokeNative]::GetWindowRect(
            $Window,
            [ref]$rectangle
        )
    ) {
        throw "Could not measure the onboarding window."
    }
    $bitmap = New-Object System.Drawing.Bitmap (
        $rectangle.Right - $rectangle.Left
    ), (
        $rectangle.Bottom - $rectangle.Top
    )
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen(
            $rectangle.Left,
            $rectangle.Top,
            0,
            0,
            $bitmap.Size
        )
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

$originalLocalAppData = $env:LOCALAPPDATA
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "gradial-onboarding-smoke-" + [guid]::NewGuid().ToString("N")
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$configPath = Join-Path $gradialDirectory "config.json"
$markerPath = Join-Path $gradialDirectory "onboarding-v1.json"
$logPath = Join-Path $gradialDirectory "logs\gradial_rCURRENT.log"
$standardOutputPath = Join-Path $temporaryRoot "gradial.stdout.log"
$standardErrorPath = Join-Path $temporaryRoot "gradial.stderr.log"
$actionId = "67726164-6961-4c00-8000-000000000a01"
$profileId = "67726164-6961-4c00-8000-000000000001"
$gradial = $null
$controlIsDown = $false
$m5IsDown = $false

try {
    New-Item -ItemType Directory -Path $gradialDirectory -Force | Out-Null
    $slices = @(
        @("N", "NE", "E", "SE", "S", "SW", "W", "NW") |
            ForEach-Object {
                [ordered]@{
                    label = $_
                    action_id = $actionId
                }
            }
    )
    $configuration = [ordered]@{
        schema_version = 3
        config_revision = 1
        activation_binding = [ordered]@{
            button = "m4"
            modifiers = [ordered]@{
                control = $false
                alt = $false
                shift = $false
            }
        }
        global_profile = [ordered]@{
            id = $profileId
            name = "Onboarding smoke"
            menu = [ordered]@{
                slice_count = 8
                slices = $slices
            }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "Must not run during onboarding"
                kind = [ordered]@{
                    type = "delay"
                    milliseconds = 50
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
    $gradial = Start-Process `
        -FilePath $executable `
        -WorkingDirectory $workspace `
        -PassThru `
        -RedirectStandardOutput $standardOutputPath `
        -RedirectStandardError $standardErrorPath `
        -WindowStyle Hidden
    $env:LOCALAPPDATA = $originalLocalAppData

    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=onboarding_state_loaded .*required=true" `
        -Failure "Missing completion state did not require onboarding." | Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=overlay_pool_ready .*config_revision=1" `
        -Failure "Gradial did not reach its initial ready state." | Out-Null

    $onboardingWindow = [IntPtr]::Zero
    $windowDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $onboardingWindow = [GradialOnboardingSmokeNative]::FindVisibleWindow(
            [uint32]$gradial.Id,
            "Welcome to Gradial"
        )
    } while (
        $onboardingWindow -eq [IntPtr]::Zero -and
        [DateTime]::UtcNow -lt $windowDeadline
    )
    if ($onboardingWindow -eq [IntPtr]::Zero) {
        throw "The required native onboarding window was not visible."
    }

    if ($CaptureScreenshot) {
        $captureDirectory = Join-Path $workspace "captures"
        New-Item -ItemType Directory -Path $captureDirectory -Force | Out-Null
        Save-WindowScreenshot `
            -Window $onboardingWindow `
            -Path (Join-Path $captureDirectory "onboarding-binding-smoke.png")
    }

    if (-not [GradialOnboardingSmokeNative]::SendKey($onboardingWindow, [uint32]0x70)) {
        throw "Could not choose and acknowledge the isolated Ctrl plus M5 chord."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=onboarding_practice_started .*config_revision=2 .*binding_changed=true .*activation_button=M5 .*activation_control=true" `
        -Failure "Onboarding did not save the chosen Ctrl plus M5 chord." | Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=overlay_pool_ready .*config_revision=2" `
        -Failure "The interaction runtime did not reload the onboarding chord." | Out-Null

    [GradialOnboardingSmokeNative]::SetCursorPos(600, 500) | Out-Null
    [GradialOnboardingSmokeNative]::SetControl($true)
    $controlIsDown = $true
    [GradialOnboardingSmokeNative]::SendM5($true)
    $m5IsDown = $true
    Start-Sleep -Milliseconds 160
    [GradialOnboardingSmokeNative]::SendRelativeMove(116, 0)
    Start-Sleep -Milliseconds 160
    [GradialOnboardingSmokeNative]::SendM5($false)
    $m5IsDown = $false
    [GradialOnboardingSmokeNative]::SetControl($false)
    $controlIsDown = $false

    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=profile_resolved .*config_revision=2 .*menu_source=onboarding_practice .*onboarding_practice=true" `
        -Failure "Practice did not use the isolated eight-slice sample menu." | Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=onboarding_practice_progress .*practice_event=selected .*opened=true .*selected=true .*cancelled=false" `
        -Failure "The safe sample selection did not reach onboarding progress." | Out-Null

    [GradialOnboardingSmokeNative]::SetControl($true)
    $controlIsDown = $true
    [GradialOnboardingSmokeNative]::SendM5($true)
    $m5IsDown = $true
    Start-Sleep -Milliseconds 120
    [GradialOnboardingSmokeNative]::SendM5($false)
    $m5IsDown = $false
    [GradialOnboardingSmokeNative]::SetControl($false)
    $controlIsDown = $false
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=onboarding_practice_progress .*practice_event=cancelled .*opened=true .*selected=true .*cancelled=true" `
        -Failure "The center cancellation did not complete onboarding practice." | Out-Null

    $logBeforeCompletion = Get-Content -LiteralPath $logPath -Raw
    if (
        $logBeforeCompletion -match "event_code=action_dispatched" -or
        $logBeforeCompletion -match "event_code=action_execution_completed"
    ) {
        throw "A configured action ran during isolated onboarding practice."
    }

    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $onboardingWindow `
            -Path (Join-Path $workspace "captures\onboarding-practice-complete-smoke.png")
    }
    if (-not [GradialOnboardingSmokeNative]::SendKey($onboardingWindow, [uint32]0x77)) {
        throw "Could not finish the completed onboarding checklist."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=onboarding_completed .*config_revision=2" `
        -Failure "Completed practice did not persist onboarding state." | Out-Null

    $marker = Get-Content -LiteralPath $markerPath -Raw | ConvertFrom-Json
    if ($marker.schema_version -ne 1 -or $marker.completed -ne $true) {
        throw "The versioned onboarding completion marker is invalid."
    }
    $saved = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    if (
        $saved.config_revision -ne 2 -or
        $saved.activation_binding.button -ne "m5" -or
        $saved.activation_binding.modifiers.control -ne $true -or
        $saved.activation_binding.modifiers.alt -ne $false -or
        $saved.activation_binding.modifiers.shift -ne $false
    ) {
        throw "The onboarding activation choice was not saved exactly."
    }

    if (-not [GradialOnboardingSmokeNative]::CloseControllerWindow([uint32]$gradial.Id)) {
        throw "Could not close the first isolated Gradial process."
    }
    Wait-Process -Id $gradial.Id -Timeout 10
    $gradial = $null

    $env:LOCALAPPDATA = $temporaryRoot
    $gradial = Start-Process `
        -FilePath $executable `
        -WorkingDirectory $workspace `
        -PassThru `
        -RedirectStandardOutput $standardOutputPath `
        -RedirectStandardError $standardErrorPath `
        -WindowStyle Hidden
    $env:LOCALAPPDATA = $originalLocalAppData
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=onboarding_state_loaded .*required=false" `
        -Failure "The completion marker did not suppress onboarding on restart." | Out-Null
    Start-Sleep -Milliseconds 500
    if (
        [GradialOnboardingSmokeNative]::FindVisibleWindow(
            [uint32]$gradial.Id,
            "Welcome to Gradial"
        ) -ne [IntPtr]::Zero
    ) {
        throw "Completed onboarding reopened on the next startup."
    }

    Write-Host (
        "Onboarding smoke check passed: required first-run window, dedicated chord, " +
        "safe production-menu practice, selection, cancellation, completion, and restart."
    )
}
finally {
    if ($m5IsDown) {
        [GradialOnboardingSmokeNative]::SendM5($false)
    }
    if ($controlIsDown) {
        [GradialOnboardingSmokeNative]::SetControl($false)
    }
    $env:LOCALAPPDATA = $originalLocalAppData
    if ($gradial -and -not $gradial.HasExited) {
        if (-not [GradialOnboardingSmokeNative]::CloseControllerWindow([uint32]$gradial.Id)) {
            Stop-Process -Id $gradial.Id -Force -ErrorAction SilentlyContinue
        }
        Wait-Process -Id $gradial.Id -Timeout 5 -ErrorAction SilentlyContinue
    }
    $resolvedTemporary = Resolve-Path -LiteralPath $temporaryRoot -ErrorAction SilentlyContinue
    if ($resolvedTemporary) {
        $resolvedTemporaryPath = $resolvedTemporary.Path
        $resolvedTempRoot = (Resolve-Path ([System.IO.Path]::GetTempPath())).Path
        if (
            $resolvedTemporaryPath.StartsWith(
                $resolvedTempRoot,
                [System.StringComparison]::OrdinalIgnoreCase
            ) -and
            (Split-Path $resolvedTemporaryPath -Leaf).StartsWith(
                "gradial-onboarding-smoke-",
                [System.StringComparison]::Ordinal
            )
        ) {
            Remove-Item -LiteralPath $resolvedTemporaryPath -Recurse -Force
        }
    }
}
