param(
    [switch]$CaptureScreenshot
)

$ErrorActionPreference = "Stop"

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Resolve-Path (Join-Path $workspace "target\debug\gradial.exe")
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the start-at-login check."
}

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class GradialStartAtLoginSmokeNative
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

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(
        IntPtr window,
        StringBuilder text,
        int maximum
    );

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(
        IntPtr window,
        uint message,
        IntPtr wparam,
        IntPtr lparam
    );

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rectangle);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

    public static IntPtr FindVisibleWindow(uint wantedProcessId, string wantedTitle)
    {
        IntPtr match = IntPtr.Zero;
        EnumWindows(delegate(IntPtr window, IntPtr data)
        {
            uint processId;
            GetWindowThreadProcessId(window, out processId);
            if (processId != wantedProcessId || !IsWindowVisible(window))
            {
                return true;
            }
            StringBuilder title = new StringBuilder(256);
            GetWindowText(window, title, title.Capacity);
            if (String.Equals(title.ToString(), wantedTitle, StringComparison.Ordinal))
            {
                match = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return match;
    }

    public static bool SendKey(IntPtr window, uint virtualKey)
    {
        return
            PostMessage(window, 0x0100u, new IntPtr(virtualKey), IntPtr.Zero) &&
            PostMessage(window, 0x0101u, new IntPtr(virtualKey), IntPtr.Zero);
    }
}
"@

function Wait-LogPattern {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string]$Pattern,
        [Parameter(Mandatory = $true)]
        [string]$Failure
    )

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        if (
            (Test-Path -LiteralPath $Path) -and
            (Select-String -LiteralPath $Path -Pattern $Pattern -Quiet)
        ) {
            return
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw $Failure
}

function Wait-RegistryValue {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Key,
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [AllowNull()]
        [AllowEmptyString()]
        [string]$Expected,
        [Parameter(Mandatory = $true)]
        [bool]$ShouldExist
    )

    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $actual = Read-RegistryValue -Key $Key -Name $Name
        $exists = $null -ne $actual
        if (
            ($ShouldExist -and $exists -and $actual -ceq $Expected) -or
            (-not $ShouldExist -and -not $exists)
        ) {
            return
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "The isolated Windows startup value did not reach the expected state."
}

function Read-RegistryValue {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Key,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    $registryKey = Get-Item -LiteralPath $Key
    return $registryKey.GetValue(
        $Name,
        $null,
        [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames
    )
}

function Save-WindowScreenshot {
    param(
        [Parameter(Mandatory = $true)]
        [IntPtr]$Window,
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    [GradialStartAtLoginSmokeNative]::SetForegroundWindow($Window) | Out-Null
    Start-Sleep -Milliseconds 250
    $rectangle = New-Object GradialStartAtLoginSmokeNative+Rect
    if (-not [GradialStartAtLoginSmokeNative]::GetWindowRect($Window, [ref]$rectangle)) {
        throw "Could not read the settings-window rectangle."
    }
    $width = $rectangle.Right - $rectangle.Left
    $height = $rectangle.Bottom - $rectangle.Top
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
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

$runKey = "Registry::HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run"
$valueName = "GradialSmoke-$([Guid]::NewGuid().ToString('N'))"
$temporaryRoot = Join-Path (
    [System.IO.Path]::GetTempPath()
) "gradial-start-at-login-smoke-$([Guid]::NewGuid().ToString('N'))"
$logPath = Join-Path $temporaryRoot "Gradial\logs\gradial_rCURRENT.log"
$standardOutputPath = Join-Path $temporaryRoot "gradial.stdout.log"
$standardErrorPath = Join-Path $temporaryRoot "gradial.stderr.log"
$expectedCommand = "`"$($executable.Path)`""
$staleCommand = '"C:\Stale Gradial\gradial.exe" --unexpected'
$originalLocalAppData = $env:LOCALAPPDATA
$originalValueName = $env:GRADIAL_STARTUP_VALUE_NAME
$productionValueBefore = Read-RegistryValue -Key $runKey -Name "Gradial"
$gradial = $null

try {
    New-Item -ItemType Directory -Path $temporaryRoot -Force | Out-Null
    if ($null -ne (Read-RegistryValue -Key $runKey -Name $valueName)) {
        throw "The unique isolated startup value already exists."
    }

    $env:LOCALAPPDATA = $temporaryRoot
    $env:GRADIAL_STARTUP_VALUE_NAME = $valueName
    $gradial = Start-Process `
        -FilePath $executable `
        -ArgumentList @("--open-settings", "--skip-onboarding") `
        -WorkingDirectory $workspace `
        -PassThru `
        -RedirectStandardOutput $standardOutputPath `
        -RedirectStandardError $standardErrorPath `
        -WindowStyle Hidden
    $env:LOCALAPPDATA = $originalLocalAppData
    $env:GRADIAL_STARTUP_VALUE_NAME = $originalValueName

    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=settings_opened .*result_code=ok" `
        -Failure "The settings editor did not open."

    $settingsWindow = [IntPtr]::Zero
    $windowDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $settingsWindow = [GradialStartAtLoginSmokeNative]::FindVisibleWindow(
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

    if (
        -not [GradialStartAtLoginSmokeNative]::SendKey(
            $settingsWindow,
            [uint32]0x86
        )
    ) {
        throw "Could not refresh the start-at-login status."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=start_at_login_status_read .*status=disabled" `
        -Failure "Gradial did not report the isolated startup value as disabled."

    if (
        -not [GradialStartAtLoginSmokeNative]::SendKey(
            $settingsWindow,
            [uint32]0x87
        )
    ) {
        throw "Could not enable start at login."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=start_at_login_changed .*action=enable status=enabled" `
        -Failure "Gradial did not enable the isolated startup value."
    Wait-RegistryValue `
        -Key $runKey `
        -Name $valueName `
        -Expected $expectedCommand `
        -ShouldExist $true

    Set-ItemProperty `
        -LiteralPath $runKey `
        -Name $valueName `
        -Value $staleCommand `
        -Type String
    if (
        -not [GradialStartAtLoginSmokeNative]::SendKey(
            $settingsWindow,
            [uint32]0x86
        )
    ) {
        throw "Could not inspect the stale start-at-login value."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=start_at_login_status_read .*status=needs_repair" `
        -Failure "Gradial did not detect the stale startup value."

    if ($CaptureScreenshot) {
        $captureDirectory = Join-Path $workspace "captures"
        New-Item -ItemType Directory -Path $captureDirectory -Force | Out-Null
        Save-WindowScreenshot `
            -Window $settingsWindow `
            -Path (Join-Path $captureDirectory "settings-start-at-login-repair-smoke.png")
    }

    if (
        -not [GradialStartAtLoginSmokeNative]::SendKey(
            $settingsWindow,
            [uint32]0x87
        )
    ) {
        throw "Could not repair start at login."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=start_at_login_changed .*action=repair status=enabled" `
        -Failure "Gradial did not repair the stale startup value."
    Wait-RegistryValue `
        -Key $runKey `
        -Name $valueName `
        -Expected $expectedCommand `
        -ShouldExist $true

    if (
        -not [GradialStartAtLoginSmokeNative]::SendKey(
            $settingsWindow,
            [uint32]0x87
        )
    ) {
        throw "Could not turn off start at login."
    }
    Wait-LogPattern `
        -Path $logPath `
        -Pattern "event_code=start_at_login_changed .*action=disable status=disabled" `
        -Failure "Gradial did not remove the isolated startup value."
    Wait-RegistryValue `
        -Key $runKey `
        -Name $valueName `
        -Expected $null `
        -ShouldExist $false

    $productionValueAfter = Read-RegistryValue -Key $runKey -Name "Gradial"
    if (
        -not [string]::Equals(
            $productionValueBefore,
            $productionValueAfter,
            [System.StringComparison]::Ordinal
        )
    ) {
        throw "The isolated check changed the production Gradial startup value."
    }

    $privateLogText = Get-Content -Raw -LiteralPath $logPath
    if (
        $privateLogText.Contains($expectedCommand) -or
        $privateLogText.Contains($staleCommand) -or
        $privateLogText.Contains($valueName)
    ) {
        throw "Start-at-login logging exposed a registry value name or command."
    }

    Write-Host (
        "Start-at-login smoke check passed: disabled, enabled with one quoted " +
        "argument-free executable, stale detection, repair, disable, and redacted logs."
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
    $env:GRADIAL_STARTUP_VALUE_NAME = $originalValueName
    Remove-ItemProperty `
        -LiteralPath $runKey `
        -Name $valueName `
        -ErrorAction SilentlyContinue

    if ($gradial -and -not $gradial.HasExited) {
        Stop-Process -Id $gradial.Id -Force
        $gradial.WaitForExit(5000) | Out-Null
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
            "gradial-start-at-login-smoke-"
        )
    ) {
        Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
    }
}
