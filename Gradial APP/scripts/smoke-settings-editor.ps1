param(
    [switch]$CaptureScreenshot
)

$ErrorActionPreference = "Stop"

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\debug\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's debug configuration before running the settings-editor smoke check."
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the isolated settings check."
}

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class GradialSettingsSmokeNative
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

    public static bool SendRecordedShortcut(IntPtr window)
    {
        return
            PostKey(window, 0x0100u, 0xA2u, 0x001D0001u) &&
            PostKey(window, 0x0100u, 0x7Du, 0x00650001u) &&
            PostKey(window, 0x0101u, 0x7Du, 0xC0650001u) &&
            PostKey(window, 0x0101u, 0xA2u, 0xC01D0001u);
    }

    private static bool PostKey(
        IntPtr window,
        uint message,
        uint virtualKey,
        uint keyData
    )
    {
        return PostMessage(
            window,
            message,
            new IntPtr(virtualKey),
            new IntPtr(unchecked((int)keyData))
        );
    }

    public static bool CloseWindow(IntPtr window)
    {
        return PostMessage(window, 0x0010u, IntPtr.Zero, IntPtr.Zero);
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

    public static int CountVisibleWindows(uint processId, string title)
    {
        var count = 0;
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (windowProcessId != processId || !IsWindowVisible(window)) return true;
            var text = new StringBuilder(256);
            GetWindowText(window, text, text.Capacity);
            if (text.ToString() == title) count++;
            return true;
        }, IntPtr.Zero);
        return count;
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

    public static void SendM4(bool down)
    {
        SendMouse(0, 0, 1, down ? 0x0080u : 0x0100u);
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
        Get-Content -LiteralPath $Path -Tail 20 | Write-Host
    }
    throw $Failure
}

function Save-WindowScreenshot {
    param(
        [IntPtr]$Window,
        [string]$Path
    )

    $rectangle = New-Object GradialSettingsSmokeNative+Rect
    if (
        -not [GradialSettingsSmokeNative]::GetWindowRect(
            $Window,
            [ref]$rectangle
        )
    ) {
        throw "Could not measure the settings window for screenshot capture."
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
    "gradial-settings-smoke-" + [guid]::NewGuid().ToString("N")
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$configPath = Join-Path $gradialDirectory "config.json"
$backupPath = Join-Path $gradialDirectory "config.json.bak"
$logPath = Join-Path $gradialDirectory "logs\gradial_rCURRENT.log"
$standardOutputPath = Join-Path $temporaryRoot "gradial.stdout.log"
$standardErrorPath = Join-Path $temporaryRoot "gradial.stderr.log"
$actionId = "67726164-6961-4c00-8000-000000000901"
$shortcutActionId = "67726164-6961-4c00-8000-000000000902"
$textActionId = "67726164-6961-4c00-8000-000000000903"
$profileId = "67726164-6961-4c00-8000-000000000001"
$labels = @("North old", "NE old", "East old", "SE old", "South old", "SW old", "West old", "NW old")
$gradial = $null
$m4IsDown = $false

try {
    New-Item -ItemType Directory -Path $gradialDirectory -Force | Out-Null
    $slices = for ($index = 0; $index -lt $labels.Count; $index++) {
        [ordered]@{
            label = $labels[$index]
            action_id = if ($index % 2 -eq 0) { $actionId } else { $null }
        }
    }
    $configuration = [ordered]@{
        schema_version = 2
        config_revision = 1
        global_profile = [ordered]@{
            id = $profileId
            name = "Settings smoke"
            menu = [ordered]@{
                slice_count = 8
                slices = $slices
            }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "Settings smoke delay"
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
        -ArgumentList "--open-settings" `
        -WorkingDirectory $workspace `
        -PassThru `
        -RedirectStandardOutput $standardOutputPath `
        -RedirectStandardError $standardErrorPath `
        -WindowStyle Hidden
    $env:LOCALAPPDATA = $originalLocalAppData

    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=overlay_pool_ready .*config_revision=1" `
        -Failure "Gradial did not reach its initial ready state." | Out-Null

    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_opened .*config_revision=1" `
        -Failure "The settings editor did not open." | Out-Null

    $settingsWindow = [IntPtr]::Zero
    $windowDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $settingsWindow = [GradialSettingsSmokeNative]::FindVisibleWindow(
            [uint32]$gradial.Id,
            "Gradial settings"
        )
    } while (
        $settingsWindow -eq [IntPtr]::Zero -and
        [DateTime]::UtcNow -lt $windowDeadline
    )
    if ($settingsWindow -eq [IntPtr]::Zero) {
        throw "The native settings window was not visible."
    }

    if ($CaptureScreenshot) {
        $captureDirectory = Join-Path $workspace "captures"
        New-Item -ItemType Directory -Path $captureDirectory -Force | Out-Null
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $captureDirectory "settings-editor-smoke.png")
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x78)) {
        throw "Could not send the action-catalog edit command to the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_action_draft_edited .*action_count=3" `
        -Failure "The action catalog did not accept the isolated shortcut and text actions." |
        Out-Null

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x79)) {
        throw "Could not start shortcut recording in the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_shortcut_recording_started .*action_id=$shortcutActionId" `
        -Failure "The shortcut recorder did not start." | Out-Null
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-shortcut-recorder-smoke.png")
    }
    if (-not [GradialSettingsSmokeNative]::SendRecordedShortcut($settingsWindow)) {
        throw "Could not send Ctrl+F14 to the shortcut recorder."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_shortcut_recorded .*action_id=$shortcutActionId .*modifier_count=1 .*scan_code=101 .*extended=false" `
        -Failure "The shortcut recorder did not capture exact Ctrl+F14 scan-code data." |
        Out-Null

    if ($CaptureScreenshot) {
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-actions-editor-smoke.png")
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x74)) {
        throw "Could not send the isolated edit command to the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_draft_edited .*slice_count=4" `
        -Failure "The settings draft did not accept the isolated edit." | Out-Null

    if (-not [GradialSettingsSmokeNative]::CloseWindow($settingsWindow)) {
        throw "Could not request a native close of the dirty settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_close_confirmation_shown .*config_revision=1" `
        -Failure "Closing a dirty draft did not show the discard confirmation." | Out-Null
    if (
        [GradialSettingsSmokeNative]::CountVisibleWindows(
            [uint32]$gradial.Id,
            "Gradial settings"
        ) -ne 1
    ) {
        throw "The dirty settings window closed before an explicit choice."
    }

    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-close-confirmation-smoke.png")
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x76)) {
        throw "Could not choose Keep editing in the close confirmation."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_close_cancelled .*config_revision=1" `
        -Failure "Keep editing did not return to the dirty draft." | Out-Null

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x77)) {
        throw "Could not send the save command to the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_saved .*config_revision=2 .*slice_count=4" `
        -Failure "The settings draft did not save atomically." | Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=overlay_pool_ready .*config_revision=2" `
        -Failure "The live interaction runtime did not reload the saved revision." | Out-Null

    $saved = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    if ($saved.config_revision -ne 2 -or $saved.global_profile.menu.slice_count -ne 4) {
        throw "The saved configuration has the wrong revision or slice count."
    }
    $expectedEditedLabels = @(
        "North edited",
        "East edited",
        "South edited",
        "West edited"
    )
    $savedLabels = @($saved.global_profile.menu.slices | ForEach-Object { $_.label })
    if ((Compare-Object $expectedEditedLabels $savedLabels -SyncWindow 0).Count -ne 0) {
        throw "The saved four-slice labels do not match the editor draft."
    }
    $savedActionIds = @($saved.global_profile.menu.slices | ForEach-Object { $_.action_id })
    if (
        $savedActionIds[0] -ne $shortcutActionId -or
        @($savedActionIds[1..3] | Where-Object { $_ -ne $actionId }).Count -ne 0
    ) {
        throw "The new shortcut and preserved cardinal action assignments are incorrect."
    }
    if (@($saved.actions).Count -ne 3) {
        throw "The settings save did not preserve all three catalog actions."
    }
    $shortcutAction = $saved.actions | Where-Object { $_.id -eq $shortcutActionId }
    if (
        -not $shortcutAction -or
        $shortcutAction.name -ne "Editor shortcut" -or
        $shortcutAction.kind.type -ne "send_shortcut" -or
        $shortcutAction.kind.key.scan_code -ne 101 -or
        $shortcutAction.kind.key.extended -ne $false -or
        @($shortcutAction.kind.modifiers).Count -ne 1 -or
        $shortcutAction.kind.modifiers[0] -ne "left_control"
    ) {
        throw "The recorded Ctrl+F14 action was not saved exactly."
    }
    $textAction = $saved.actions | Where-Object { $_.id -eq $textActionId }
    if (
        -not $textAction -or
        $textAction.name -ne "Editor text" -or
        $textAction.kind.type -ne "type_text" -or
        $textAction.kind.text -ne "Gradial editor text 🙂"
    ) {
        throw "The Unicode text action was not saved exactly."
    }
    $backup = Get-Content -LiteralPath $backupPath -Raw | ConvertFrom-Json
    if (
        $backup.config_revision -ne 1 -or
        $backup.global_profile.menu.slice_count -ne 8 -or
        @($backup.actions).Count -ne 1
    ) {
        throw "The atomic save did not preserve the previous eight-slice configuration."
    }

    if (-not [GradialSettingsSmokeNative]::CloseWindow($settingsWindow)) {
        throw "Could not request a native close of the clean settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_closed .*config_revision=2 .*unsaved_changes_discarded=false" `
        -Failure "The clean settings editor did not close without a prompt." | Out-Null
    Start-Sleep -Milliseconds 250
    if (
        [GradialSettingsSmokeNative]::CountVisibleWindows(
            [uint32]$gradial.Id,
            "Gradial settings"
        ) -ne 0
    ) {
        throw "The clean native settings window remained visible after close."
    }

    [GradialSettingsSmokeNative]::SetCursorPos(600, 500) | Out-Null
    [GradialSettingsSmokeNative]::SendM4($true)
    $m4IsDown = $true
    Start-Sleep -Milliseconds 180
    [GradialSettingsSmokeNative]::SendRelativeMove(116, 0)
    Start-Sleep -Milliseconds 180
    [GradialSettingsSmokeNative]::SendM4($false)
    $m4IsDown = $false

    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=profile_resolved .*config_revision=2 .*menu_slice_count=4 .*menu_label_count=4" `
        -Failure "A fresh invocation did not use the saved four-slice menu." | Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=action_execution_completed .*result_code=ok .*config_revision=2" `
        -Failure "The preserved cardinal action did not execute from the reloaded menu." | Out-Null

    Write-Host (
        "Settings editor smoke check passed: action creation, exact Ctrl+F14 recording, " +
        "Unicode text, guarded dirty close, atomic revision 2 save, and live reload."
    )
}
catch {
    if ($gradial -and $gradial.HasExited) {
        Write-Host "Gradial exited with code $($gradial.ExitCode)."
    }
    if (Test-Path -LiteralPath $standardOutputPath) {
        Get-Content -LiteralPath $standardOutputPath -Tail 30 | Write-Host
    }
    if (Test-Path -LiteralPath $standardErrorPath) {
        Get-Content -LiteralPath $standardErrorPath -Tail 30 | Write-Host
    }
    throw
}
finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    if ($m4IsDown) {
        [GradialSettingsSmokeNative]::SendM4($false)
    }
    if ($gradial -and -not $gradial.HasExited) {
        if (
            -not [GradialSettingsSmokeNative]::CloseControllerWindow(
                [uint32]$gradial.Id
            )
        ) {
            Stop-Process -Id $gradial.Id -Force
        }
        elseif (-not $gradial.WaitForExit(5000)) {
            Stop-Process -Id $gradial.Id -Force
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
        (Split-Path -Leaf $resolvedTemporary.Path).StartsWith(
            "gradial-settings-smoke-"
        )
    ) {
        Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
    }
}
