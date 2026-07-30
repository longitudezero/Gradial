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
$importPath = Join-Path $gradialDirectory "settings-import-smoke.json"
$exportDirectory = Join-Path $gradialDirectory "settings-export-smoke"
$exportPath = Join-Path $exportDirectory "settings.json"
$iconSourcePath = Join-Path $gradialDirectory "settings-icon-smoke.png"
$logPath = Join-Path $gradialDirectory "logs\gradial_rCURRENT.log"
$standardOutputPath = Join-Path $temporaryRoot "gradial.stdout.log"
$standardErrorPath = Join-Path $temporaryRoot "gradial.stderr.log"
$actionId = "67726164-6961-4c00-8000-000000000901"
$shortcutActionId = "67726164-6961-4c00-8000-000000000902"
$textActionId = "67726164-6961-4c00-8000-000000000903"
$urlActionId = "67726164-6961-4c00-8000-000000000905"
$pathActionId = "67726164-6961-4c00-8000-000000000906"
$executableActionId = "67726164-6961-4c00-8000-000000000907"
$delayActionId = "67726164-6961-4c00-8000-000000000908"
$windowActionId = "67726164-6961-4c00-8000-000000000909"
$sequenceActionId = "67726164-6961-4c00-8000-000000000910"
$nestedSequenceActionId = "67726164-6961-4c00-8000-000000000911"
$applicationProfileId = "67726164-6961-4c00-8000-000000000920"
$profileTestTitle = "Profile test title private 7F3C"
$profileId = "67726164-6961-4c00-8000-000000000001"
$labels = @("North old", "NE old", "East old", "SE old", "South old", "SW old", "West old", "NW old")
$gradial = $null
$m4IsDown = $false
$m5IsDown = $false
$controlIsDown = $false

try {
    New-Item -ItemType Directory -Path $gradialDirectory -Force | Out-Null
    New-Item -ItemType Directory -Path $exportDirectory -Force | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $gradialDirectory "onboarding-v1.json"),
        '{"schema_version":1,"completed":true}',
        [System.Text.UTF8Encoding]::new($false)
    )
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
    $importConfiguration = (
        $configuration | ConvertTo-Json -Depth 10 | ConvertFrom-Json
    )
    $importConfiguration.config_revision = 999
    $importConfiguration.global_profile.name = "Imported settings private 9A4D"
    $importConfiguration.global_profile.menu.slices[0].label = "Imported north private 9A4D"
    [System.IO.File]::WriteAllText(
        $importPath,
        ($importConfiguration | ConvertTo-Json -Depth 10),
        [System.Text.UTF8Encoding]::new($false)
    )
    $iconBitmap = New-Object System.Drawing.Bitmap 48, 48
    $iconGraphics = [System.Drawing.Graphics]::FromImage($iconBitmap)
    $iconBrush = New-Object System.Drawing.SolidBrush (
        [System.Drawing.Color]::FromArgb(255, 224, 96, 72)
    )
    try {
        $iconGraphics.Clear([System.Drawing.Color]::Transparent)
        $iconGraphics.FillEllipse($iconBrush, 4, 4, 40, 40)
        $iconBitmap.Save($iconSourcePath, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $iconBrush.Dispose()
        $iconGraphics.Dispose()
        $iconBitmap.Dispose()
    }

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

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x7E)) {
        throw "Could not open the native settings import chooser."
    }
    $pickerWindow = [IntPtr]::Zero
    $pickerDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $pickerWindow = [GradialSettingsSmokeNative]::FindVisibleWindow(
            [uint32]$gradial.Id,
            "Gradial: import settings"
        )
    } while (
        $pickerWindow -eq [IntPtr]::Zero -and
        [DateTime]::UtcNow -lt $pickerDeadline
    )
    if ($pickerWindow -eq [IntPtr]::Zero) {
        throw "The native Windows settings import chooser was not visible."
    }
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 500
        Save-WindowScreenshot `
            -Window $pickerWindow `
            -Path (Join-Path $workspace "captures\settings-native-import-chooser-smoke.png")
    }
    if (-not [GradialSettingsSmokeNative]::CloseWindow($pickerWindow)) {
        throw "Could not cancel the native settings import chooser."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_native_picker .*result_code=cancelled .*picker_kind=config_import" `
        -Failure "The native settings import chooser did not report safe cancellation." |
        Out-Null

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x7F)) {
        throw "Could not open the native settings export chooser."
    }
    $pickerWindow = [IntPtr]::Zero
    $pickerDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $pickerWindow = [GradialSettingsSmokeNative]::FindVisibleWindow(
            [uint32]$gradial.Id,
            "Gradial: export settings"
        )
    } while (
        $pickerWindow -eq [IntPtr]::Zero -and
        [DateTime]::UtcNow -lt $pickerDeadline
    )
    if ($pickerWindow -eq [IntPtr]::Zero) {
        throw "The native Windows settings export chooser was not visible."
    }
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 500
        Save-WindowScreenshot `
            -Window $pickerWindow `
            -Path (Join-Path $workspace "captures\settings-native-export-chooser-smoke.png")
    }
    if (-not [GradialSettingsSmokeNative]::CloseWindow($pickerWindow)) {
        throw "Could not cancel the native settings export chooser."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_native_picker .*result_code=cancelled .*picker_kind=config_export" `
        -Failure "The native settings export chooser did not report safe cancellation." |
        Out-Null

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x7C)) {
        throw "Could not import the isolated settings fixture."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_imported_to_draft .*result_code=ok .*config_revision=1 .*action_count=1 .*migration_from=2" `
        -Failure "The settings import did not migrate into the private draft." |
        Out-Null
    $activeAfterImport = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    if (
        $activeAfterImport.schema_version -ne 2 -or
        $activeAfterImport.config_revision -ne 1 -or
        $activeAfterImport.global_profile.menu.slices[0].label -ne "North old" -or
        $null -ne $activeAfterImport.activation_binding
    ) {
        throw "Import changed the active configuration before Save."
    }
    $sourceAfterImport = Get-Content -LiteralPath $importPath -Raw | ConvertFrom-Json
    if (
        $sourceAfterImport.schema_version -ne 2 -or
        $sourceAfterImport.config_revision -ne 999 -or
        $sourceAfterImport.global_profile.menu.slices[0].label -ne "Imported north private 9A4D" -or
        $null -ne $sourceAfterImport.activation_binding
    ) {
        throw "Import changed its source file instead of retaining literal schema-two data."
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x80)) {
        throw "Could not apply the appearance draft in the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_appearance_draft_edited .*accent_red=224 .*accent_green=96 .*accent_blue=72 .*label_theme=dark .*size_preset=large" `
        -Failure "The appearance editor did not retain its color, label, and size draft." |
        Out-Null
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-appearance-editor-smoke.png")
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x81)) {
        throw "Could not import the icon fixture into the settings draft."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_icon_import_requested .*result_code=ok .*slice_index=0" `
        -Failure "The icon editor did not request the isolated fixture." | Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=icon_imported .*result_code=ok .*format=png .*width=48 .*height=48" `
        -Failure "The icon importer did not validate, copy, and cache the PNG fixture." |
        Out-Null
    $managedIcons = @(
        Get-ChildItem -LiteralPath (Join-Path $gradialDirectory "icons") -Filter "*.png"
    )
    if ($managedIcons.Count -ne 1) {
        throw "The icon importer did not create exactly one managed PNG source."
    }
    $managedIconId = $managedIcons[0].BaseName
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-icon-editor-smoke.png")
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x70)) {
        throw "Could not configure and arm the activation-binding tester."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_binding_draft_edited .*activation_button=M5 .*activation_control=true .*tester_armed=true" `
        -Failure "The activation editor did not retain the M5 plus Ctrl draft." |
        Out-Null
    [GradialSettingsSmokeNative]::SetControl($true)
    $controlIsDown = $true
    [GradialSettingsSmokeNative]::SendM5($true)
    $m5IsDown = $true
    Start-Sleep -Milliseconds 100
    [GradialSettingsSmokeNative]::SendRelativeMove(116, 0)
    Start-Sleep -Milliseconds 100
    [GradialSettingsSmokeNative]::SendM5($false)
    $m5IsDown = $false
    [GradialSettingsSmokeNative]::SetControl($false)
    $controlIsDown = $false
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_binding_test_completed .*result_code=ok .*matched=true .*activation_button=M5 .*activation_control=true .*selection=E" `
        -Failure "The safe tester did not recognize the complete Ctrl plus M5 east flick." |
        Out-Null
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-binding-editor-smoke.png")
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x78)) {
        throw "Could not send the action-catalog edit command to the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_action_draft_edited .*action_count=10" `
        -Failure "The action catalog did not accept all isolated leaf-action payloads." |
        Out-Null

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x73)) {
        throw "Could not send the profile edit command to the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_profile_draft_edited .*profile_count=1 .*profile_id=$applicationProfileId .*priority=72 .*slice_count=8" `
        -Failure "The application-profile editor did not preserve its isolated profile." |
        Out-Null
    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x71)) {
        throw "Could not start the profile-test countdown."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_profile_test_capture_completed .*executable_path_available=true .*window_class_available=true .*window_title_available=true" `
        -Failure "The profile tester did not capture the foreground window after its countdown." |
        Out-Null
    Start-Sleep -Milliseconds 100
    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x72)) {
        throw "Could not show the private profile-test result."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_profile_test_shown .*executable_path_available=true .*window_class_available=true .*window_title_available=true" `
        -Failure "The profile tester did not resolve its private captured target." |
        Out-Null
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-profile-editor-smoke.png")
    }
    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x78)) {
        throw "Could not return to the action editor after the profile check."
    }
    Start-Sleep -Milliseconds 150

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

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x75)) {
        throw "Could not select the application editor in the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_leaf_editor_selected .*action_id=$executableActionId" `
        -Failure "The application payload editor did not become active." | Out-Null
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-leaf-action-editor-smoke.png")
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x7A)) {
        throw "Could not open the native application chooser."
    }
    $pickerWindow = [IntPtr]::Zero
    $pickerDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $pickerWindow = [GradialSettingsSmokeNative]::FindVisibleWindow(
            [uint32]$gradial.Id,
            "Gradial: choose an application"
        )
    } while (
        $pickerWindow -eq [IntPtr]::Zero -and
        [DateTime]::UtcNow -lt $pickerDeadline
    )
    if ($pickerWindow -eq [IntPtr]::Zero) {
        throw "The native Windows application chooser was not visible."
    }
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 500
        Save-WindowScreenshot `
            -Window $pickerWindow `
            -Path (Join-Path $workspace "captures\settings-native-application-chooser-smoke.png")
    }
    if (-not [GradialSettingsSmokeNative]::CloseWindow($pickerWindow)) {
        throw "Could not cancel the native application chooser."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_native_picker .*result_code=cancelled .*picker_kind=application" `
        -Failure "The native application chooser did not report safe cancellation." | Out-Null

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x7B)) {
        throw "Could not select the nested sequence editor."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_sequence_editor_selected .*action_id=$nestedSequenceActionId" `
        -Failure "The nested sequence editor did not become active." | Out-Null
    if ($CaptureScreenshot) {
        Start-Sleep -Milliseconds 200
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $workspace "captures\settings-sequence-editor-smoke.png")
    }

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x74)) {
        throw "Could not send the isolated edit command to the settings window."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_debug_draft_edited .*slice_count=4" `
        -Failure "The settings draft did not accept the isolated edit." | Out-Null

    if (-not [GradialSettingsSmokeNative]::SendKey($settingsWindow, [uint32]0x7D)) {
        throw "Could not export the current unsaved settings draft."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_draft_exported .*result_code=ok .*config_revision=1 .*action_count=10 .*icon_referenced_count=1 .*icon_copied_count=1 .*icon_reused_count=0" `
        -Failure "The complete unsaved settings draft was not exported." |
        Out-Null
    $exported = Get-Content -LiteralPath $exportPath -Raw | ConvertFrom-Json
    $exportedApplication = $exported.actions |
        Where-Object { $_.id -eq $executableActionId }
    if (
        $exported.schema_version -ne 5 -or
        $exported.config_revision -ne 1 -or
        $exported.global_profile.menu.slice_count -ne 4 -or
        @($exported.actions).Count -ne 10 -or
        $exported.appearance.accent_color.red -ne 224 -or
        $exported.appearance.accent_color.green -ne 96 -or
        $exported.appearance.accent_color.blue -ne 72 -or
        $exported.appearance.label_theme -ne "dark" -or
        $exported.appearance.size_preset -ne "large" -or
        $exported.global_profile.menu.slices[0].icon.asset_id -ne $managedIconId -or
        $exported.global_profile.menu.slices[0].icon.format -ne "png" -or
        $exported.activation_binding.button -ne "m5" -or
        $exported.activation_binding.modifiers.control -ne $true -or
        -not $exportedApplication -or
        $exportedApplication.kind.executable -ne "C:\Windows\System32\notepad.exe" -or
        $exportedApplication.kind.working_directory -ne "C:\Windows" -or
        (Compare-Object `
            @("--editor-smoke", "literal argument with spaces", "") `
            @($exportedApplication.kind.arguments) `
            -SyncWindow 0
        ).Count -ne 0
    ) {
        throw "Export did not preserve the valid draft, revision, or literal application fields."
    }
    $portableIconPath = Join-Path (
        Join-Path $exportDirectory "icons"
    ) ($managedIconId + ".png")
    if (
        -not (Test-Path -LiteralPath $portableIconPath -PathType Leaf) -or
        (Get-FileHash -LiteralPath $portableIconPath -Algorithm SHA256).Hash -ne
            (Get-FileHash -LiteralPath $managedIcons[0].FullName -Algorithm SHA256).Hash
    ) {
        throw "Export did not copy the referenced icon into its portable icons directory."
    }
    $activeAfterExport = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    if (
        $activeAfterExport.schema_version -ne 2 -or
        $activeAfterExport.config_revision -ne 1 -or
        $activeAfterExport.global_profile.menu.slice_count -ne 8
    ) {
        throw "Export changed the active configuration before Save."
    }

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
        -Pattern "event_code=settings_saved .*config_revision=2 .*slice_count=4 .*accent_red=224 .*accent_green=96 .*accent_blue=72 .*label_theme=dark .*size_preset=large .*icon_assignment_count=2" `
        -Failure "The settings draft did not save atomically." | Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=renderer_appearance_updated .*config_revision=2 .*accent_red=224 .*accent_green=96 .*accent_blue=72 .*label_theme=dark .*size_preset=large" `
        -Failure "The production renderer did not accept the saved appearance." | Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=overlay_pool_ready .*config_revision=2" `
        -Failure "The live interaction runtime did not reload the saved revision." | Out-Null

    $saved = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    if (
        $saved.schema_version -ne 5 -or
        $saved.config_revision -ne 2 -or
        $saved.global_profile.menu.slice_count -ne 4
    ) {
        throw "The saved configuration has the wrong schema, revision, or slice count."
    }
    if (
        $saved.activation_binding.button -ne "m5" -or
        $saved.activation_binding.modifiers.control -ne $true -or
        $saved.activation_binding.modifiers.alt -ne $false -or
        $saved.activation_binding.modifiers.shift -ne $false
    ) {
        throw "The exact Ctrl plus M5 activation binding was not saved."
    }
    if (
        $saved.appearance.accent_color.red -ne 224 -or
        $saved.appearance.accent_color.green -ne 96 -or
        $saved.appearance.accent_color.blue -ne 72 -or
        $saved.appearance.label_theme -ne "dark" -or
        $saved.appearance.size_preset -ne "large"
    ) {
        throw "The saved appearance does not match the editor draft."
    }
    if (
        $saved.global_profile.menu.slices[0].icon.asset_id -ne $managedIconId -or
        $saved.global_profile.menu.slices[0].icon.format -ne "png" -or
        -not (Test-Path -LiteralPath (
            Join-Path $gradialDirectory (
                "icons\" + $managedIconId + ".png"
            )
        ))
    ) {
        throw "The saved north slice does not reference the managed PNG icon."
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
    if (@($saved.actions).Count -ne 10) {
        throw "The settings save did not preserve all ten catalog actions."
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
    $urlAction = $saved.actions | Where-Object { $_.id -eq $urlActionId }
    if (
        -not $urlAction -or
        $urlAction.name -ne "Editor website" -or
        $urlAction.kind.type -ne "open_url" -or
        $urlAction.kind.url -ne "https://example.com/gradial-editor"
    ) {
        throw "The website action was not saved exactly."
    }
    $pathAction = $saved.actions | Where-Object { $_.id -eq $pathActionId }
    if (
        -not $pathAction -or
        $pathAction.name -ne "Editor folder" -or
        $pathAction.kind.type -ne "open_path" -or
        $pathAction.kind.path -ne "C:\Windows"
    ) {
        throw "The file-or-folder action was not saved exactly."
    }
    $executableAction = $saved.actions | Where-Object { $_.id -eq $executableActionId }
    $expectedArguments = @("--editor-smoke", "literal argument with spaces", "")
    $savedArguments = @($executableAction.kind.arguments)
    if (
        -not $executableAction -or
        $executableAction.name -ne "Editor application" -or
        $executableAction.kind.type -ne "run_executable" -or
        $executableAction.kind.executable -ne "C:\Windows\System32\notepad.exe" -or
        $executableAction.kind.working_directory -ne "C:\Windows" -or
        (Compare-Object $expectedArguments $savedArguments -SyncWindow 0).Count -ne 0
    ) {
        throw "The application path, working folder, or literal argument rows changed during save."
    }
    $delayAction = $saved.actions | Where-Object { $_.id -eq $delayActionId }
    if (
        -not $delayAction -or
        $delayAction.name -ne "Editor delay" -or
        $delayAction.kind.type -ne "delay" -or
        $delayAction.kind.milliseconds -ne 275
    ) {
        throw "The Delay action was not saved exactly."
    }
    $windowAction = $saved.actions | Where-Object { $_.id -eq $windowActionId }
    if (
        -not $windowAction -or
        $windowAction.name -ne "Editor window" -or
        $windowAction.kind.type -ne "window" -or
        $windowAction.kind.command -ne "restore"
    ) {
        throw "The invocation-window action was not saved exactly."
    }
    $sequenceAction = $saved.actions | Where-Object { $_.id -eq $sequenceActionId }
    $expectedSequenceSteps = @($shortcutActionId, $textActionId, $delayActionId)
    if (
        -not $sequenceAction -or
        $sequenceAction.name -ne "Editor sequence" -or
        $sequenceAction.kind.type -ne "sequence" -or
        (Compare-Object $expectedSequenceSteps @($sequenceAction.kind.actions) -SyncWindow 0).Count -ne 0
    ) {
        throw "The ordered base sequence was not saved exactly."
    }
    $nestedSequenceAction = $saved.actions |
        Where-Object { $_.id -eq $nestedSequenceActionId }
    $expectedNestedSteps = @($windowActionId, $sequenceActionId)
    if (
        -not $nestedSequenceAction -or
        $nestedSequenceAction.name -ne "Editor nested sequence" -or
        $nestedSequenceAction.kind.type -ne "sequence" -or
        (Compare-Object $expectedNestedSteps @($nestedSequenceAction.kind.actions) -SyncWindow 0).Count -ne 0
    ) {
        throw "The reordered nested sequence was not saved exactly."
    }
    if (@($saved.application_profiles).Count -ne 1) {
        throw "The settings save did not preserve exactly one application profile."
    }
    $applicationProfile = $saved.application_profiles[0]
    if (
        $applicationProfile.profile.id -ne $applicationProfileId -or
        $applicationProfile.profile.name -ne "Editor Notepad" -or
        $applicationProfile.rule.executable_path -ne "C:\Windows\System32\notepad.exe" -or
        $applicationProfile.rule.priority -ne 72 -or
        $applicationProfile.rule.window_class -ne "Notepad" -or
        $applicationProfile.rule.title_regex -ne "^Profile test title" -or
        $applicationProfile.profile.menu.slice_count -ne 8 -or
        @($applicationProfile.profile.menu.slices).Count -ne 8 -or
        $applicationProfile.profile.menu.slices[1].label -ne "NE old" -or
        $applicationProfile.profile.menu.slices[0].icon.asset_id -ne $managedIconId -or
        $applicationProfile.profile.menu.slices[0].icon.format -ne "png" -or
        $applicationProfile.profile.menu.slices[2].label -ne "Profile east" -or
        $applicationProfile.profile.menu.slices[2].action_id -ne $nestedSequenceActionId
    ) {
        throw "The independent application profile, exact rule, priority, or menu was not saved exactly."
    }
    $backup = Get-Content -LiteralPath $backupPath -Raw | ConvertFrom-Json
    if (
        $backup.schema_version -ne 2 -or
        $backup.config_revision -ne 1 -or
        $backup.global_profile.menu.slice_count -ne 8 -or
        @($backup.actions).Count -ne 1 -or
        $null -ne $backup.activation_binding
    ) {
        throw "The atomic save did not preserve the previous schema-two configuration."
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
    [GradialSettingsSmokeNative]::SetControl($true)
    $controlIsDown = $true
    [GradialSettingsSmokeNative]::SendM5($true)
    $m5IsDown = $true
    Start-Sleep -Milliseconds 180
    [GradialSettingsSmokeNative]::SendRelativeMove(116, 0)
    Start-Sleep -Milliseconds 180
    [GradialSettingsSmokeNative]::SendM5($false)
    $m5IsDown = $false
    [GradialSettingsSmokeNative]::SetControl($false)
    $controlIsDown = $false

    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=invocation_snapshot_captured .*config_revision=2 .*modifier_control=true .*modifier_alt=false .*modifier_shift=false .*activation_binding=MouseButton5" `
        -Failure "The fresh invocation did not freeze the saved Ctrl plus M5 binding." |
        Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=profile_resolved .*config_revision=2 .*menu_slice_count=4 .*menu_label_count=4" `
        -Failure "A fresh Ctrl plus M5 invocation did not use the saved four-slice menu." |
        Out-Null
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=action_execution_completed .*result_code=ok .*config_revision=2" `
        -Failure "The preserved cardinal action did not execute from the reloaded menu." | Out-Null

    $logText = Get-Content -LiteralPath $logPath -Raw
    foreach ($privateValue in @(
        $profileTestTitle,
        "Imported settings private 9A4D",
        "Imported north private 9A4D",
        "settings-import-smoke.json",
        "settings-export-smoke",
        "settings-icon-smoke.png",
        "C:\Windows\System32\notepad.exe",
        "Notepad"
    )) {
        if ($logText.Contains($privateValue)) {
            throw "The settings editor wrote a private imported or captured value to Gradial's log."
        }
    }

    Write-Host (
        "Settings editor smoke check passed: safe Ctrl plus M5 hold-flick-release test, " +
        "private import, appearance and icon previews, managed PNG import, portable draft " +
        "export, advanced profiles, all action editors, schema-five save, and live reload."
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
    if ($m5IsDown) {
        [GradialSettingsSmokeNative]::SendM5($false)
    }
    if ($controlIsDown) {
        [GradialSettingsSmokeNative]::SetControl($false)
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
