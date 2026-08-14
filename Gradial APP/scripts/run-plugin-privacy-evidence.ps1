param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\captures\plugin-privacy"),

    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.IO.Compression.FileSystem

function Write-Utf8File {
    param(
        [string]$Path,
        [string]$Contents
    )

    [IO.File]::WriteAllText($Path, $Contents, [Text.UTF8Encoding]::new($false))
}

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

function Get-Sha256Text {
    param([string]$Text)

    return ([BitConverter]::ToString((
        Get-Sha256Bytes ([Text.Encoding]::UTF8.GetBytes($Text))
    ))).Replace("-", "").ToLowerInvariant()
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
    return ([BitConverter]::ToString((
        Get-Sha256Bytes $packagePayload
    ))).Replace("-", "").ToLowerInvariant()
}

function Wait-Path {
    param(
        [string]$Path,
        [string]$Failure,
        [int]$Seconds = 10
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        if (Test-Path -LiteralPath $Path -PathType Leaf) {
            return
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw $Failure
}

function Wait-LogPattern {
    param(
        [string]$Directory,
        [string]$Pattern,
        [string]$Failure,
        [int]$Seconds = 10
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    do {
        Start-Sleep -Milliseconds 100
        $text = Get-CombinedLogs $Directory
        if ($text -match $Pattern) {
            return $text
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw $Failure
}

function Get-CombinedLogs {
    param([string]$Directory)

    if (-not (Test-Path -LiteralPath $Directory -PathType Container)) {
        return ""
    }
    return (@(
        Get-ChildItem -LiteralPath $Directory -Filter "gradial*.log" |
            Sort-Object LastWriteTime, Name |
            ForEach-Object { Get-Content -Raw -LiteralPath $_.FullName }
    ) -join [Environment]::NewLine)
}

function Read-ZipEntries {
    param([string]$Path)

    $entries = [ordered]@{}
    $archive = [IO.Compression.ZipFile]::OpenRead($Path)
    try {
        foreach ($entry in $archive.Entries) {
            if ($entries.Contains($entry.FullName)) {
                throw "Diagnostic ZIP contains a duplicate $($entry.FullName) entry."
            }
            $reader = [IO.StreamReader]::new(
                $entry.Open(),
                [Text.UTF8Encoding]::new($false, $true),
                $true
            )
            try {
                $entries[$entry.FullName] = $reader.ReadToEnd()
            }
            finally {
                $reader.Dispose()
            }
        }
    }
    finally {
        $archive.Dispose()
    }
    return $entries
}

function Assert-PluginPrivacySurfaces {
    param(
        [string]$RawLog,
        [Collections.IDictionary]$DefaultEntries,
        [Collections.IDictionary]$OptInEntries,
        [Collections.IDictionary]$Sentinels,
        [string]$OptInPath
    )

    if (
        $RawLog -notmatch (
            "event_code=plugin_lifecycle result_code=ok error_code=none " +
            "plugin_id=org\.gradial\.privacy .*stage=commit"
        ) -or
        $RawLog -notmatch "event_code=plugin_notification_queued result_code=ok plugin_id=org\.gradial\.privacy" -or
        $RawLog -notmatch "event_code=plugin_notification_shown result_code=ok plugin_id=org\.gradial\.privacy" -or
        $RawLog -notmatch (
            "event_code=plugin_lifecycle result_code=failure " +
            "error_code=lua_runtime_error plugin_id=org\.gradial\.privacy .*stage=commit"
        )
    ) {
        throw "The raw log omitted required success, capability, or redacted Lua-failure evidence."
    }

    foreach ($entry in $Sentinels.GetEnumerator()) {
        if (
            $RawLog.IndexOf(
                [string]$entry.Value,
                [StringComparison]::OrdinalIgnoreCase
            ) -ge 0
        ) {
            throw "Raw release logs exposed the $($entry.Key) sentinel."
        }
    }

    if (-not $DefaultEntries.Contains("manifest.json")) {
        throw "The default diagnostic bundle omitted its manifest."
    }
    if ($DefaultEntries.Contains("configured-process-paths.json")) {
        throw "The default diagnostic bundle included configured process paths."
    }
    $defaultLogEntries = @(
        $DefaultEntries.Keys | Where-Object { $_ -match '^logs/gradial_r(?:CURRENT|[0-9]{5})\.log$' }
    )
    if (
        $defaultLogEntries.Count -eq 0 -or
        $defaultLogEntries.Count -gt 5 -or
        $DefaultEntries.Count -ne (1 + $defaultLogEntries.Count)
    ) {
        throw "The default diagnostic bundle contains an unexpected entry."
    }
    $defaultManifest = $DefaultEntries["manifest.json"] | ConvertFrom-Json
    if (
        $defaultManifest.privacy.logs_sanitized -ne $true -or
        $defaultManifest.privacy.configured_process_paths_included -ne $false -or
        $defaultManifest.privacy.configured_process_path_count -ne 0
    ) {
        throw "The default diagnostic manifest recorded the wrong privacy decisions."
    }
    foreach ($entry in $DefaultEntries.GetEnumerator()) {
        foreach ($sentinel in $Sentinels.GetEnumerator()) {
            if (
                ([string]$entry.Value).IndexOf(
                    [string]$sentinel.Value,
                    [StringComparison]::OrdinalIgnoreCase
                ) -ge 0
            ) {
                throw "The default bundle exposed the $($sentinel.Key) sentinel in $($entry.Key)."
            }
        }
    }

    if (-not $OptInEntries.Contains("configured-process-paths.json")) {
        throw "The opted-in diagnostic bundle omitted configured process paths."
    }
    $optInLogEntries = @(
        $OptInEntries.Keys | Where-Object { $_ -match '^logs/gradial_r(?:CURRENT|[0-9]{5})\.log$' }
    )
    if (
        $optInLogEntries.Count -eq 0 -or
        $optInLogEntries.Count -gt 5 -or
        $OptInEntries.Count -ne (2 + $optInLogEntries.Count)
    ) {
        throw "The opted-in diagnostic bundle contains an unexpected entry."
    }
    $optInManifest = $OptInEntries["manifest.json"] | ConvertFrom-Json
    if (
        $optInManifest.privacy.logs_sanitized -ne $true -or
        $optInManifest.privacy.configured_process_paths_included -ne $true -or
        $optInManifest.privacy.configured_process_path_count -ne 1
    ) {
        throw "The opted-in diagnostic manifest recorded the wrong privacy decisions."
    }
    $pathDocument = $OptInEntries["configured-process-paths.json"] | ConvertFrom-Json
    if (@($pathDocument.paths).Count -ne 1 -or $pathDocument.paths[0] -ne $OptInPath) {
        throw "The opted-in process-path document did not contain the exact configured path."
    }
    foreach ($entry in $OptInEntries.GetEnumerator()) {
        foreach ($sentinel in $Sentinels.GetEnumerator()) {
            $allowed = (
                $sentinel.Key -eq "configured_process_path" -and
                $entry.Key -eq "configured-process-paths.json"
            )
            if (
                -not $allowed -and
                ([string]$entry.Value).IndexOf(
                    [string]$sentinel.Value,
                    [StringComparison]::OrdinalIgnoreCase
                ) -ge 0
            ) {
                throw "The opted-in bundle exposed the $($sentinel.Key) sentinel in $($entry.Key)."
            }
        }
    }

    return @(
        "release_log_payload_exclusion",
        "release_log_lua_failure_redaction",
        "default_bundle_closed_entries",
        "default_bundle_sentinel_exclusion",
        "opt_in_bundle_exact_process_path",
        "opt_in_bundle_non_path_exclusion"
    )
}

function Invoke-SelfTest {
    $sentinels = [ordered]@{
        plugin_parameter = "PRIVATE-PARAM-SELFTEST"
        lua_failure = "PRIVATE-FAILURE-SELFTEST"
        configured_process_path = "PRIVATE-PATH-SELFTEST"
    }
    $safeLog = @"
event_code=plugin_lifecycle result_code=ok error_code=none plugin_id=org.gradial.privacy session_id=1 stage=commit
event_code=plugin_notification_queued result_code=ok plugin_id=org.gradial.privacy
event_code=plugin_notification_shown result_code=ok plugin_id=org.gradial.privacy
event_code=plugin_lifecycle result_code=failure error_code=lua_runtime_error plugin_id=org.gradial.privacy session_id=2 stage=commit
"@
    $defaultEntries = [ordered]@{
        "manifest.json" = '{"privacy":{"logs_sanitized":true,"configured_process_paths_included":false,"configured_process_path_count":0}}'
        "logs/gradial_rCURRENT.log" = $safeLog
    }
    $optInPath = "C:\Privacy\PRIVATE-PATH-SELFTEST\tool.exe"
    $optInEntries = [ordered]@{
        "manifest.json" = '{"privacy":{"logs_sanitized":true,"configured_process_paths_included":true,"configured_process_path_count":1}}'
        "logs/gradial_rCURRENT.log" = $safeLog
        "configured-process-paths.json" = (@{
            notice = "explicit opt in"
            paths = @($optInPath)
        } | ConvertTo-Json)
    }
    $checks = Assert-PluginPrivacySurfaces `
        $safeLog `
        $defaultEntries `
        $optInEntries `
        $sentinels `
        $optInPath
    if ($checks.Count -ne 6) {
        throw "The privacy analyzer self-test returned an incomplete check set."
    }
    $caught = $false
    try {
        Assert-PluginPrivacySurfaces `
            ($safeLog + $sentinels.plugin_parameter) `
            $defaultEntries `
            $optInEntries `
            $sentinels `
            $optInPath | Out-Null
    }
    catch {
        $caught = $true
    }
    if (-not $caught) {
        throw "The privacy analyzer accepted an injected raw-log sentinel."
    }
    $caught = $false
    try {
        $leakingDefault = [ordered]@{}
        foreach ($entry in $defaultEntries.GetEnumerator()) {
            $leakingDefault[$entry.Key] = $entry.Value
        }
        $leakingDefault["logs/gradial_rCURRENT.log"] += $sentinels.lua_failure
        Assert-PluginPrivacySurfaces `
            $safeLog `
            $leakingDefault `
            $optInEntries `
            $sentinels `
            $optInPath | Out-Null
    }
    catch {
        $caught = $true
    }
    if (-not $caught) {
        throw "The privacy analyzer accepted an injected default-bundle sentinel."
    }
    $caught = $false
    try {
        $leakingOptIn = [ordered]@{}
        foreach ($entry in $optInEntries.GetEnumerator()) {
            $leakingOptIn[$entry.Key] = $entry.Value
        }
        $leakingOptIn["logs/gradial_rCURRENT.log"] += $sentinels.configured_process_path
        Assert-PluginPrivacySurfaces `
            $safeLog `
            $defaultEntries `
            $leakingOptIn `
            $sentinels `
            $optInPath | Out-Null
    }
    catch {
        $caught = $true
    }
    if (-not $caught) {
        throw "The privacy analyzer accepted an opted-in path sentinel outside its path document."
    }
    $caught = $false
    try {
        $unexpectedDefault = [ordered]@{}
        foreach ($entry in $defaultEntries.GetEnumerator()) {
            $unexpectedDefault[$entry.Key] = $entry.Value
        }
        $unexpectedDefault["config.json"] = "{}"
        Assert-PluginPrivacySurfaces `
            $safeLog `
            $unexpectedDefault `
            $optInEntries `
            $sentinels `
            $optInPath | Out-Null
    }
    catch {
        $caught = $true
    }
    if (-not $caught) {
        throw "The privacy analyzer accepted an unexpected default-bundle entry."
    }
    Write-Host "Plugin privacy analyzer self-test passed."
}

function Remove-TemporaryRoot {
    param([string]$Path)

    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction SilentlyContinue
    $temporary = Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())
    if (
        $resolved -and
        $resolved.Path.StartsWith($temporary.Path, [StringComparison]::OrdinalIgnoreCase) -and
        (Split-Path -Leaf $resolved.Path).StartsWith("gradial-plugin-privacy-")
    ) {
        Remove-Item -LiteralPath $resolved.Path -Recurse -Force
    }
}

if ($SelfTest) {
    Invoke-SelfTest
    return
}

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$releaseExecutable = Join-Path $workspace "target\release\gradial.exe"
$debugExecutable = Join-Path $workspace "target\debug\gradial.exe"
if (
    -not (Test-Path -LiteralPath $releaseExecutable -PathType Leaf) -or
    -not (Test-Path -LiteralPath $debugExecutable -PathType Leaf)
) {
    throw "Build Gradial's debug and release configurations before running privacy evidence."
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running isolated privacy evidence."
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class GradialPluginPrivacyNative
{
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr data);

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

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr window, StringBuilder text, int maximum);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr window);

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
            Rect location;
            if (
                Shell_NotifyIconGetRect(ref identifier, out location) == 0 &&
                PostMessage(window, 0x0010u, IntPtr.Zero, IntPtr.Zero)
            ) {
                closed = true;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return closed;
    }
}
"@

$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) (
    "gradial-plugin-privacy-" + [guid]::NewGuid().ToString("N")
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$pluginId = "org.gradial.privacy"
$pluginDirectory = Join-Path $gradialDirectory "plugins\$pluginId"
$logDirectory = Join-Path $gradialDirectory "logs"
$configPath = Join-Path $gradialDirectory "config.json"
$statePath = Join-Path $gradialDirectory "plugins-state.json"
$defaultBundlePath = Join-Path $temporaryRoot "gradial-diagnostics-default-smoke.zip"
$optInBundlePath = Join-Path $temporaryRoot "gradial-diagnostics-paths-smoke.zip"
$loopbackReadyPath = Join-Path $temporaryRoot "loopback-ready.txt"
$loopbackRequestPath = Join-Path $temporaryRoot "loopback-request.bin"
$targetReadyPath = Join-Path $temporaryRoot "target-ready.txt"
$targetEventPath = Join-Path $temporaryRoot "target-events.txt"
$settingsStdoutPath = Join-Path $temporaryRoot "settings.stdout.txt"
$settingsStderrPath = Join-Path $temporaryRoot "settings.stderr.txt"
$targetProcess = $null
$loopbackProcess = $null
$settingsProcess = $null
$originalLocalAppData = $env:LOCALAPPDATA
$succeeded = $false

$sentinels = [ordered]@{
    plugin_parameter = "PRIVATE-PARAMETER-Q7M4-29VC"
    plugin_source = "PRIVATE-SOURCE-P8K2-41RX"
    plugin_manifest = "PRIVATE-MANIFEST-D6H9-73WF"
    context_window_title = "PRIVATE-WINDOW-T4N8-52BZ"
    context_application_path = "PRIVATE-APPLICATION-J5R3-84LP"
    shortcut_value = "left_control+f14"
    notification_payload = "PRIVATE NOTIFICATION C4V7 61KS"
    loopback_path = "PRIVATE-LOOPBACK-PATH-B9F2-37XN"
    loopback_body = "PRIVATE-LOOPBACK-BODY-A3D8-95MU"
    lua_failure = "PRIVATE-LUA-FAILURE-E2W6-48GY"
    configured_process_path = "PRIVATE-CONFIGURED-PATH-L7C5-26QT"
}

try {
    New-Item -ItemType Directory -Path $pluginDirectory -Force | Out-Null
    Write-Utf8File `
        (Join-Path $gradialDirectory "onboarding-v1.json") `
        '{"schema_version":1,"completed":true}'

    $portProbe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    try {
        $portProbe.Start()
        $loopbackPort = ([Net.IPEndPoint]$portProbe.LocalEndpoint).Port
    }
    finally {
        $portProbe.Stop()
    }
    $origin = "http://127.0.0.1:$loopbackPort"

    $targetExecutable = Join-Path $temporaryRoot (
        $sentinels.context_application_path + ".exe"
    )
    Add-Type `
        -ReferencedAssemblies @("System.Windows.Forms", "System.Drawing") `
        -OutputAssembly $targetExecutable `
        -OutputType WindowsApplication `
        -TypeDefinition @"
using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Windows.Forms;

public sealed class GradialPrivacyTarget : Form
{
    private readonly string eventPath;
    private readonly string readyPath;

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    public GradialPrivacyTarget(string eventPath, string readyPath, string title)
    {
        this.eventPath = eventPath;
        this.readyPath = readyPath;
        Text = title;
        Width = 420;
        Height = 120;
        StartPosition = FormStartPosition.CenterScreen;
        FormBorderStyle = FormBorderStyle.FixedToolWindow;
        Controls.Add(new Label {
            AutoSize = true,
            Left = 18,
            Top = 22,
            Text = "Isolated Gradial privacy target"
        });
        Shown += (_, __) => {
            Activate();
            SetForegroundWindow(Handle);
            File.WriteAllText(readyPath, Process.GetCurrentProcess().Id + "," + Handle.ToInt64());
        };
    }

    protected override void WndProc(ref Message message)
    {
        const int WmKeyDown = 0x0100;
        const int WmKeyUp = 0x0101;
        const int WmSysKeyDown = 0x0104;
        const int WmSysKeyUp = 0x0105;
        if (
            message.Msg == WmKeyDown || message.Msg == WmKeyUp ||
            message.Msg == WmSysKeyDown || message.Msg == WmSysKeyUp
        ) {
            long encoded = message.LParam.ToInt64();
            int scanCode = (int)((encoded >> 16) & 0xFF);
            bool keyUp = message.Msg == WmKeyUp || message.Msg == WmSysKeyUp;
            File.AppendAllText(
                eventPath,
                message.Msg + "," + message.WParam.ToInt64() + "," + scanCode + "," +
                keyUp.ToString().ToLowerInvariant() + Environment.NewLine
            );
        }
        base.WndProc(ref message);
    }
}

public static class GradialPrivacyTargetProgram
{
    [STAThread]
    public static void Main(string[] args)
    {
        Application.EnableVisualStyles();
        Application.Run(new GradialPrivacyTarget(args[0], args[1], args[2]));
    }
}
"@

    $manifest = @"
manifest_version = 1
id = "$pluginId"
name = "Privacy evidence fixture"
version = "0.1.0"
api_version = 1
entry = "main.lua"
permissions = ["context.application", "context.window_title", "input.shortcut", "notification", "network.loopback"]
loopback = [{ origin = "$origin", methods = ["POST"] }]

[[actions]]
id = "capabilities"
name = "Privacy capability evidence"
kind = "continuous"
description = "$($sentinels.plugin_manifest)"
update_hz = 30

[[actions.parameters]]
key = "private_parameter"
label = "Private parameter"
type = "string"
required = true
minimum_length = 1
maximum_length = 128

[[actions]]
id = "failure"
name = "Privacy failure evidence"
kind = "continuous"
update_hz = 30

[[actions.parameters]]
key = "private_parameter"
label = "Private parameter"
type = "string"
required = true
minimum_length = 1
maximum_length = 128
"@
    $source = @"
local source_private = "$($sentinels.plugin_source)"

return {
    begin = function(context, parameters, initial)
        assert(source_private ~= nil)
        assert(parameters.private_parameter == "$($sentinels.plugin_parameter)")
        assert(context.window_title == "$($sentinels.context_window_title)")
        assert(context.application ~= nil)
        assert(string.find(context.application.executable_path, "$($sentinels.context_application_path)", 1, true) ~= nil)
        assert(context.application.window_class ~= "")
        if context.action_id == "capabilities" then
            local accepted, reason = gradial.input.shortcut("$($sentinels.shortcut_value)")
            assert(accepted and reason == nil)
        end
        return {
            action_id = context.action_id,
            private_parameter = parameters.private_parameter,
            window_title = context.window_title,
            executable_path = context.application.executable_path,
            value = initial.value,
        }
    end,
    update = function(session, sample)
        session.value = sample.value
    end,
    commit = function(session, sample)
        session.value = sample.value
        if session.action_id == "failure" then
            error("$($sentinels.lua_failure)")
        end
        local notified, notification_reason = gradial.notification("$($sentinels.notification_payload)")
        assert(notified and notification_reason == nil)
        local body = session.private_parameter .. "|" .. session.window_title .. "|" ..
            session.executable_path .. "|$($sentinels.loopback_body)"
        local posted, loopback_reason = gradial.network.loopback({
            method = "POST",
            origin = "$origin",
            path = "/privacy/$($sentinels.loopback_path)",
            body = body,
            content_type = "text/plain; charset=utf-8",
        })
        assert(posted and loopback_reason == nil)
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
    $record = [ordered]@{
        enabled = $true
        approved_permissions = @(
            "context.application",
            "context.window_title",
            "input.shortcut",
            "notification",
            "network.loopback"
        )
        approved_loopback = @(
            [ordered]@{ origin = $origin; methods = @("POST") }
        )
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
    $pluginRecords[$pluginId] = $record
    Write-Utf8File $statePath (([ordered]@{
        schema_version = 1
        revision = 1
        plugins = $pluginRecords
    }) | ConvertTo-Json -Depth 10)

    $profileId = "67726164-6961-4c00-8000-000000000501"
    $delayActionId = "67726164-6961-4c00-8000-000000000502"
    $pluginActionId = "67726164-6961-4c00-8000-000000000503"
    $processActionId = "67726164-6961-4c00-8000-000000000504"
    $optInPath = "C:\Program Files\$($sentinels.configured_process_path)\tool.exe"

    function Write-PrivacyConfiguration {
        param(
            [string]$PluginAction,
            [uint64]$Revision
        )

        $slices = @("N", "NE", "E", "SE", "S", "SW", "W", "NW") |
            ForEach-Object {
                [ordered]@{
                    label = $_
                    action_id = $delayActionId
                    continuous_action_id = $pluginActionId
                }
            }
        $configuration = [ordered]@{
            schema_version = 6
            config_revision = $Revision
            activation_binding = [ordered]@{
                button = "m4"
                modifiers = [ordered]@{ control = $false; alt = $false; shift = $false }
            }
            appearance = [ordered]@{
                accent_color = [ordered]@{ red = 124; green = 154; blue = 255 }
                label_theme = "light"
                size_preset = "standard"
            }
            continuous_full_scale_radius_dip = 300
            global_profile = [ordered]@{
                id = $profileId
                name = "Privacy fixture"
                menu = [ordered]@{ slice_count = 8; slices = @($slices) }
            }
            application_profiles = @()
            actions = @(
                [ordered]@{
                    id = $delayActionId
                    name = "Privacy discrete delay"
                    kind = [ordered]@{ type = "delay"; milliseconds = 10 }
                },
                [ordered]@{
                    id = $pluginActionId
                    name = "Privacy plugin action"
                    kind = [ordered]@{
                        type = "plugin_continuous"
                        plugin_id = $pluginId
                        plugin_action_id = $PluginAction
                        parameters = [ordered]@{
                            private_parameter = [ordered]@{
                                type = "string"
                                value = $sentinels.plugin_parameter
                            }
                        }
                    }
                },
                [ordered]@{
                    id = $processActionId
                    name = "Explicit path evidence"
                    kind = [ordered]@{
                        type = "run_executable"
                        executable = $optInPath
                        arguments = @()
                        working_directory = $null
                    }
                }
            )
        }
        Write-Utf8File $configPath ($configuration | ConvertTo-Json -Depth 15)
    }

    Write-PrivacyConfiguration "capabilities" 1

    $loopbackProcess = Start-Process `
        -FilePath "powershell.exe" `
        -ArgumentList @(
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
            ('"' + (Join-Path $PSScriptRoot "plugin-privacy-loopback-target.ps1") + '"'),
            "-Port", $loopbackPort,
            "-RequestPath", ('"' + $loopbackRequestPath + '"'),
            "-ReadyPath", ('"' + $loopbackReadyPath + '"')
        ) `
        -PassThru `
        -WindowStyle Hidden
    Wait-Path $loopbackReadyPath "The isolated loopback evidence target did not become ready."

    $targetProcess = Start-Process `
        -FilePath $targetExecutable `
        -ArgumentList @(
            ('"' + $targetEventPath + '"'),
            ('"' + $targetReadyPath + '"'),
            ('"' + $sentinels.context_window_title + '"')
        ) `
        -PassThru
    Wait-Path $targetReadyPath "The isolated foreground evidence target did not become ready."
    $targetReady = (Get-Content -Raw -LiteralPath $targetReadyPath).Split(',')
    $targetWindow = [IntPtr]::new([int64]$targetReady[1])

    $env:LOCALAPPDATA = $temporaryRoot
    [GradialPluginPrivacyNative]::SetForegroundWindow($targetWindow) | Out-Null
    Start-Sleep -Milliseconds 200
    & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
        -Configuration release `
        -Geometry fixed-origin `
        -Slices 8 `
        -Direction E `
        -UseConfiguredMenu `
        -PluginContinuous `
        -ContinuousPercent 75 `
        -GestureDelayMilliseconds 180
    Wait-Path $loopbackRequestPath "The plugin's private loopback payload did not reach the target."
    Wait-Path $targetEventPath "The plugin's private shortcut did not reach the target."
    $loopbackRequest = [Text.Encoding]::UTF8.GetString(
        [IO.File]::ReadAllBytes($loopbackRequestPath)
    )
    foreach ($required in @(
        $sentinels.plugin_parameter,
        $sentinels.context_window_title,
        $sentinels.context_application_path,
        $sentinels.loopback_path,
        $sentinels.loopback_body
    )) {
        if (-not $loopbackRequest.Contains($required)) {
            throw "The loopback target did not receive a required private value."
        }
    }
    $shortcutDeadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        Start-Sleep -Milliseconds 50
        $targetEvents = Get-Content -Raw -LiteralPath $targetEventPath
        $shortcutPressed = $targetEvents -match '(?m)^(256|260),[^,]+,101,false\r?$'
        $shortcutReleased = $targetEvents -match '(?m)^(257|261),[^,]+,101,true\r?$'
    } while (
        (-not $shortcutPressed -or -not $shortcutReleased) -and
        [DateTime]::UtcNow -lt $shortcutDeadline
    )
    if (-not $shortcutPressed -or -not $shortcutReleased) {
        throw "The foreground target did not receive and release the F14 shortcut key. Observed: $targetEvents"
    }

    Write-PrivacyConfiguration "failure" 2
    [GradialPluginPrivacyNative]::SetForegroundWindow($targetWindow) | Out-Null
    Start-Sleep -Milliseconds 200
    & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
        -Configuration release `
        -Geometry fixed-origin `
        -Slices 8 `
        -Direction E `
        -UseConfiguredMenu `
        -PluginContinuous `
        -ContinuousPercent 75 `
        -GestureDelayMilliseconds 180

    $stateAfterFailure = Get-Content -Raw -LiteralPath $statePath | ConvertFrom-Json
    if (-not $stateAfterFailure.plugins.$pluginId.faulted) {
        throw "The deliberate Lua failure did not persist the fixture's faulted state."
    }

    $settingsProcess = Start-Process `
        -FilePath $debugExecutable `
        -ArgumentList "--open-settings" `
        -WorkingDirectory $workspace `
        -RedirectStandardOutput $settingsStdoutPath `
        -RedirectStandardError $settingsStderrPath `
        -PassThru `
        -WindowStyle Hidden
    Wait-LogPattern `
        $logDirectory `
        "event_code=settings_opened .*config_revision=2" `
        "The diagnostic settings window did not open." | Out-Null
    $settingsWindow = [IntPtr]::Zero
    $windowDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 100
        $settingsWindow = [GradialPluginPrivacyNative]::FindVisibleWindow(
            [uint32]$settingsProcess.Id,
            "Gradial settings"
        )
    } while (
        $settingsWindow -eq [IntPtr]::Zero -and
        [DateTime]::UtcNow -lt $windowDeadline
    )
    if ($settingsWindow -eq [IntPtr]::Zero) {
        throw "The diagnostic settings window was not visible."
    }
    if (-not [GradialPluginPrivacyNative]::SendKey($settingsWindow, [uint32]0x82)) {
        throw "Could not freeze the diagnostic privacy snapshot."
    }
    Wait-LogPattern `
        $logDirectory `
        "event_code=diagnostic_preview_ready .*result_code=ok .*config_revision=2" `
        "The diagnostic privacy preview was not prepared." | Out-Null
    if (-not [GradialPluginPrivacyNative]::SendKey($settingsWindow, [uint32]0x83)) {
        throw "Could not export the default diagnostic privacy bundle."
    }
    Wait-Path $defaultBundlePath "The default diagnostic privacy bundle was not created."
    if (-not [GradialPluginPrivacyNative]::SendKey($settingsWindow, [uint32]0x84)) {
        throw "Could not export the opted-in diagnostic privacy bundle."
    }
    Wait-Path $optInBundlePath "The opted-in diagnostic privacy bundle was not created."
    if (-not [GradialPluginPrivacyNative]::CloseControllerWindow([uint32]$settingsProcess.Id)) {
        throw "Could not close Gradial after diagnostic privacy export."
    }
    if (-not $settingsProcess.WaitForExit(10000)) {
        throw "Gradial did not stop after diagnostic privacy export."
    }

    $rawLog = Get-CombinedLogs $logDirectory
    $defaultEntries = Read-ZipEntries $defaultBundlePath
    $optInEntries = Read-ZipEntries $optInBundlePath
    $checks = Assert-PluginPrivacySurfaces `
        $rawLog `
        $defaultEntries `
        $optInEntries `
        $sentinels `
        $optInPath

    $resolvedOutput = New-Item -ItemType Directory -Path $OutputDirectory -Force
    $rawLogOutput = Join-Path $resolvedOutput "release-and-export-logs.txt"
    $defaultOutput = Join-Path $resolvedOutput "diagnostics-default.zip"
    $optInOutput = Join-Path $resolvedOutput "diagnostics-process-path-opt-in.zip"
    Write-Utf8File $rawLogOutput $rawLog
    Copy-Item -LiteralPath $defaultBundlePath -Destination $defaultOutput -Force
    Copy-Item -LiteralPath $optInBundlePath -Destination $optInOutput -Force
    $sentinelHashes = [ordered]@{}
    foreach ($entry in $sentinels.GetEnumerator()) {
        $sentinelHashes[$entry.Key] = Get-Sha256Text ([string]$entry.Value)
    }
    $report = [ordered]@{
        report_version = 1
        generated_at = [DateTimeOffset]::UtcNow.ToString("o")
        result = "pass"
        route = [ordered]@{
            interaction_build = "release"
            diagnostic_export_build = "debug"
            plugin_id = $pluginId
            loopback_origin_kind = "literal_ipv4"
        }
        exercised = [ordered]@{
            parameter_crossed_lua = $true
            application_context_crossed_lua = $true
            window_title_crossed_lua = $true
            shortcut_reached_foreground_target = $true
            notification_reached_native_tray = $true
            loopback_reached_local_target = $true
            lua_runtime_failure_persisted_fault = $true
        }
        checks = @($checks | ForEach-Object {
            [ordered]@{ name = $_; result = "pass" }
        })
        sentinel_sha256 = $sentinelHashes
        diagnostic_entries = [ordered]@{
            default = @($defaultEntries.Keys)
            process_path_opt_in = @($optInEntries.Keys)
        }
        explicit_opt_in = [ordered]@{
            configured_process_path_count = 1
            configured_process_path_sha256 = Get-Sha256Text $optInPath
            non_path_sentinel_count = $sentinels.Count - 1
        }
        artifacts = [ordered]@{
            raw_logs = "release-and-export-logs.txt"
            default_bundle = "diagnostics-default.zip"
            process_path_opt_in_bundle = "diagnostics-process-path-opt-in.zip"
        }
    }
    Write-Utf8File `
        (Join-Path $resolvedOutput "report.json") `
        ($report | ConvertTo-Json -Depth 12)
    $succeeded = $true
    Write-Host (
        "Plugin privacy evidence passed: $($checks.Count) surface checks, " +
        "$($sentinels.Count) recognizable private values, one explicit path opt-in."
    )
}
finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    foreach ($process in @($settingsProcess, $targetProcess, $loopbackProcess)) {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    Remove-TemporaryRoot $temporaryRoot
    if (-not $succeeded) {
        Write-Warning "Plugin privacy evidence did not complete; its disposable fixture was removed."
    }
}
