param(
    [ValidateSet("Development", "Release")]
    [string]$Mode = "Development",

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\captures\performance"),

    [string]$PresentMonPath,

    [ValidateRange(0, 15)]
    [int]$MonitorIndex = 0,

    [switch]$MarkersOnly,

    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

function Get-NearestRankPercentile {
    param(
        [double[]]$Values,
        [ValidateRange(1, 100)]
        [int]$Percentile
    )

    if (-not $Values -or $Values.Count -eq 0) {
        return $null
    }
    $ordered = @($Values | Sort-Object)
    $rank = [Math]::Ceiling(($Percentile / 100.0) * $ordered.Count)
    return [double]$ordered[[Math]::Max(0, $rank - 1)]
}

function Get-Statistics {
    param([double[]]$Values)

    if (-not $Values -or $Values.Count -eq 0) {
        return $null
    }
    return [ordered]@{
        sample_count = $Values.Count
        p50 = Get-NearestRankPercentile $Values 50
        p95 = Get-NearestRankPercentile $Values 95
        p99 = Get-NearestRankPercentile $Values 99
        max = ($Values | Measure-Object -Maximum).Maximum
    }
}

function Read-TimingMarkers {
    param([string]$Path)

    if (-not $Path -or -not (Test-Path -LiteralPath $Path)) {
        return @()
    }
    $markers = @()
    foreach ($row in (Import-Csv -LiteralPath $Path)) {
        $text = ($row.PSObject.Properties.Value | ForEach-Object { [string]$_ }) -join " "
        $match = [regex]::Match(
            $text,
            "marker=(?<marker>[a-z_]+)\s+invocation_id=(?<invocation>[0-9a-f-]+|none)\s+" +
            "source_qpc=(?<source>-?[0-9]+)\s+marker_qpc=(?<observed>-?[0-9]+)\s+" +
            "qpc_frequency=(?<frequency>[0-9]+)"
        )
        if ($match.Success) {
            $markers += [pscustomobject]@{
                marker = $match.Groups["marker"].Value
                invocation_id = $match.Groups["invocation"].Value
                source_qpc = [int64]$match.Groups["source"].Value
                marker_qpc = [int64]$match.Groups["observed"].Value
                qpc_frequency = [int64]$match.Groups["frequency"].Value
                raw = $text
            }
        }
    }
    return @($markers)
}

function Get-QpcDurationMilliseconds {
    param(
        [int64]$Start,
        [int64]$End,
        [int64]$Frequency
    )

    if ($Start -le 0 -or $End -lt $Start -or $Frequency -le 0) {
        return $null
    }
    return (($End - $Start) * 1000.0) / $Frequency
}

function Get-MarkerDurations {
    param(
        [object[]]$Markers,
        [string]$StartMarker,
        [ValidateSet("source_qpc", "marker_qpc")]
        [string]$StartField,
        [string]$EndMarker,
        [ValidateSet("source_qpc", "marker_qpc")]
        [string]$EndField
    )

    $durations = @()
    foreach ($group in ($Markers | Where-Object {
        $_.invocation_id -ne "none"
    } | Group-Object invocation_id)) {
        $start = $group.Group | Where-Object marker -eq $StartMarker | Select-Object -First 1
        $end = $group.Group | Where-Object marker -eq $EndMarker | Select-Object -First 1
        if ($start -and $end) {
            $duration = Get-QpcDurationMilliseconds `
                -Start $start.$StartField `
                -End $end.$EndField `
                -Frequency $start.qpc_frequency
            if ($null -ne $duration) {
                $durations += $duration
            }
        }
    }
    return [double[]]$durations
}

function Get-PresentEvents {
    param(
        [string]$Path,
        [int64]$QpcFrequency
    )

    if (-not $Path -or -not (Test-Path -LiteralPath $Path)) {
        return @()
    }
    $rows = @(Import-Csv -LiteralPath $Path)
    if ($rows.Count -eq 0) {
        return @()
    }
    $names = @($rows[0].PSObject.Properties.Name)
    $qpcColumn = @("CPUStartQPC", "CPUStartQpc") |
        Where-Object { $names -contains $_ } |
        Select-Object -First 1
    $displayColumn = @("msUntilDisplayed", "MsUntilDisplayed") |
        Where-Object { $names -contains $_ } |
        Select-Object -First 1
    if (-not $qpcColumn -or -not $displayColumn -or $QpcFrequency -le 0) {
        return @()
    }

    $events = @()
    foreach ($row in $rows) {
        $startText = [string]$row.$qpcColumn
        $displayText = [string]$row.$displayColumn
        [int64]$start = 0
        [double]$displayMs = 0
        if (
            [int64]::TryParse($startText, [ref]$start) -and
            [double]::TryParse(
                $displayText,
                [Globalization.NumberStyles]::Float,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$displayMs
            ) -and
            $displayMs -ge 0
        ) {
            $events += [pscustomobject]@{
                start_qpc = $start
                displayed_qpc = $start + [int64][Math]::Round(
                    $displayMs * $QpcFrequency / 1000.0
                )
            }
        }
    }
    return @($events | Sort-Object start_qpc)
}

function Get-DisplayedDurations {
    param(
        [object[]]$Markers,
        [object[]]$PresentEvents,
        [string]$ReadyMarker,
        [string]$InputMarker
    )

    $durations = @()
    foreach ($group in ($Markers | Where-Object {
        $_.invocation_id -ne "none"
    } | Group-Object invocation_id)) {
        $ready = $group.Group | Where-Object marker -eq $ReadyMarker | Select-Object -First 1
        $input = $group.Group | Where-Object marker -eq $InputMarker | Select-Object -First 1
        if (-not $ready -or -not $input) {
            continue
        }
        $present = $PresentEvents |
            Where-Object { $_.start_qpc -ge $ready.marker_qpc } |
            Select-Object -First 1
        if ($present) {
            $duration = Get-QpcDurationMilliseconds `
                -Start $input.source_qpc `
                -End $present.displayed_qpc `
                -Frequency $input.qpc_frequency
            if ($null -ne $duration) {
                $durations += $duration
            }
        }
    }
    return [double[]]$durations
}

function Get-GateResult {
    param(
        [Collections.IDictionary]$Statistics,
        [bool]$ProtocolEligible,
        [double]$P95Limit,
        [double]$P99Limit,
        [switch]$P95Inclusive,
        [switch]$P99Inclusive
    )

    if (-not $Statistics -or -not $ProtocolEligible) {
        return "not_evaluated"
    }
    $p95Pass = if ($P95Inclusive) {
        $Statistics.p95 -le $P95Limit
    }
    else {
        $Statistics.p95 -lt $P95Limit
    }
    $p99Pass = if ($P99Inclusive) {
        $Statistics.p99 -le $P99Limit
    }
    else {
        $Statistics.p99 -lt $P99Limit
    }
    if ($p95Pass -and $p99Pass) { return "pass" }
    return "fail"
}

function New-Metric {
    param(
        [string]$Name,
        [string]$Unit,
        [string]$MeasurementStatus,
        [string]$GateResult = "not_evaluated",
        [object]$Statistics = $null,
        [string]$Evidence = "",
        [string]$Reason = ""
    )

    return [ordered]@{
        name = $Name
        unit = $Unit
        measurement_status = $MeasurementStatus
        gate_result = $GateResult
        statistics = $Statistics
        evidence = $Evidence
        reason = $Reason
    }
}

function Write-ReportFiles {
    param(
        [Collections.IDictionary]$Report,
        [string]$Directory
    )

    $jsonPath = Join-Path $Directory "performance-report.json"
    $csvPath = Join-Path $Directory "performance-metrics.csv"
    $markdownPath = Join-Path $Directory "performance-report.md"
    [System.IO.File]::WriteAllText(
        $jsonPath,
        ($Report | ConvertTo-Json -Depth 12),
        [System.Text.UTF8Encoding]::new($false)
    )

    $flatMetrics = foreach ($metric in $Report.metrics) {
        [pscustomobject]@{
            name = $metric.name
            unit = $metric.unit
            measurement_status = $metric.measurement_status
            gate_result = $metric.gate_result
            sample_count = $metric.statistics.sample_count
            p50 = $metric.statistics.p50
            p95 = $metric.statistics.p95
            p99 = $metric.statistics.p99
            max = $metric.statistics.max
            evidence = $metric.evidence
            reason = $metric.reason
        }
    }
    $flatMetrics | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding utf8

    $lines = @(
        "# Gradial performance report",
        "",
        "- Captured: $($Report.captured_at)",
        "- Mode: $($Report.protocol.mode)",
        "- Overall gate result: **$($Report.overall_gate_result)**",
        "- Protocol eligible: $($Report.protocol.gate_eligible)",
        "- PresentMon display evidence: $($Report.capabilities.presentmon)",
        "- Power plan stable: $($Report.validity.power_plan_stable)",
        "- Foreground stable: $($Report.validity.foreground_stable)",
        "",
        "| Metric | Status | Gate | Samples | p50 | p95 | p99 | max | Unit |",
        "|---|---:|---:|---:|---:|---:|---:|---:|---|"
    )
    function Format-MetricValue {
        param([object]$Value)

        if ($null -eq $Value) {
            return "-"
        }
        return [Math]::Round([double]$Value, 3)
    }
    foreach ($metric in $Report.metrics) {
        $statistics = $metric.statistics
        $values = if ($statistics) {
            @(
                $statistics.sample_count,
                (Format-MetricValue $statistics.p50),
                (Format-MetricValue $statistics.p95),
                (Format-MetricValue $statistics.p99),
                (Format-MetricValue $statistics.max)
            )
        }
        else {
            @("-", "-", "-", "-", "-")
        }
        $lines += (
            "| $($metric.name) | $($metric.measurement_status) | " +
            "$($metric.gate_result) | $($values[0]) | $($values[1]) | " +
            "$($values[2]) | $($values[3]) | $($values[4]) | $($metric.unit) |"
        )
    }
    $lines += @(
        "",
        "## Environment",
        "",
        "- Windows: $($Report.machine.windows)",
        "- CPU: $($Report.machine.cpu)",
        "- GPU: $($Report.machine.gpu)",
        "- Logical processors: $($Report.machine.logical_processors)",
        "- Displays: $($Report.machine.displays -join '; ')",
        "- Active power plan: $($Report.machine.power_plan)",
        "",
        "## Notes",
        ""
    )
    foreach ($note in $Report.notes) {
        $lines += "- $note"
    }
    [System.IO.File]::WriteAllLines(
        $markdownPath,
        $lines,
        [System.Text.UTF8Encoding]::new($false)
    )

    return [ordered]@{
        json = $jsonPath
        csv = $csvPath
        markdown = $markdownPath
    }
}

if ($SelfTest) {
    $values = [double[]](1..100)
    $statistics = Get-Statistics $values
    if (
        $statistics.sample_count -ne 100 -or
        $statistics.p50 -ne 50 -or
        $statistics.p95 -ne 95 -or
        $statistics.p99 -ne 99 -or
        $statistics.max -ne 100
    ) {
        throw "Performance report percentile self-test failed."
    }
    $sampleMarker = [pscustomobject]@{
        marker = "activation_dequeued"
        invocation_id = "00000000-0000-0000-0000-000000000001"
        source_qpc = 100
        marker_qpc = 110
        qpc_frequency = 1000
    }
    $sampleEnd = [pscustomobject]@{
        marker = "overlay_shown"
        invocation_id = $sampleMarker.invocation_id
        source_qpc = 100
        marker_qpc = 125
        qpc_frequency = 1000
    }
    $durations = Get-MarkerDurations `
        -Markers @($sampleMarker, $sampleEnd) `
        -StartMarker "activation_dequeued" `
        -StartField source_qpc `
        -EndMarker "overlay_shown" `
        -EndField marker_qpc
    if ($durations.Count -ne 1 -or $durations[0] -ne 25) {
        throw "Performance report QPC self-test failed."
    }
    Write-Host "Performance report analyzer self-test passed."
    return
}

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\release\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's release configuration before running a performance report."
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the isolated performance report."
}

$principal = New-Object Security.Principal.WindowsPrincipal(
    [Security.Principal.WindowsIdentity]::GetCurrent()
)
$isElevated = $principal.IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator
)
if (-not $PresentMonPath) {
    $presentMonCandidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\PresentMon\PresentMon.exe"),
        "C:\Program Files\PresentMon\PresentMon.exe",
        "C:\Program Files\NVIDIA Corporation\FrameViewSDK\bin\PresentMon_x64.exe"
    )
    $PresentMonPath = $presentMonCandidates |
        Where-Object { Test-Path -LiteralPath $_ } |
        Select-Object -First 1
}
$presentMonAvailable = [bool](
    $PresentMonPath -and (Test-Path -LiteralPath $PresentMonPath)
)
$usePresentMon = $presentMonAvailable -and $isElevated -and -not $MarkersOnly

$preset = if ($Mode -eq "Release") {
    [ordered]@{
        invocation_count = 1000
        warmup_seconds = 300
        idle_seconds = 1800
        gesture_delay_ms = 10
        inter_invocation_delay_ms = 25
        cold_launch_samples = 100
        sample_interval_ms = 1000
    }
}
else {
    [ordered]@{
        invocation_count = 20
        warmup_seconds = 2
        idle_seconds = 8
        gesture_delay_ms = 10
        inter_invocation_delay_ms = 25
        cold_launch_samples = 3
        sample_interval_ms = 250
    }
}
$captureDuration = [Math]::Min(
    600,
    [Math]::Max(
        5,
        [Math]::Ceiling(
            (
                $preset.invocation_count *
                ((2 * $preset.gesture_delay_ms) + $preset.inter_invocation_delay_ms)
            ) / 1000.0
        ) + 4
    )
)
$gateEligible = (
    $Mode -eq "Release" -and
    $preset.invocation_count -ge 1000 -and
    $preset.warmup_seconds -ge 300 -and
    $preset.idle_seconds -ge 600
)

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$reportDirectory = New-Item -ItemType Directory -Path (
    Join-Path $OutputDirectory "$timestamp-$($Mode.ToLowerInvariant())"
) -Force
$captureDirectory = New-Item -ItemType Directory -Path (
    Join-Path $reportDirectory "capture"
) -Force
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "gradial-performance-" + [guid]::NewGuid().ToString("N")
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$processIdFile = Join-Path $reportDirectory "gradial.pid"
$phaseFile = Join-Path $reportDirectory "phase.txt"
$foregroundProcessFile = Join-Path $reportDirectory "foreground.pid"
$stdoutPath = Join-Path $reportDirectory "interaction.stdout.txt"
$stderrPath = Join-Path $reportDirectory "interaction.stderr.txt"
$resourcePath = Join-Path $reportDirectory "resources.csv"
$originalLocalAppData = $env:LOCALAPPDATA
$child = $null

Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class GradialPerformanceNative
{
    [StructLayout(LayoutKind.Sequential)]
    private struct Point
    {
        public int X;
        public int Y;
    }

    private delegate bool EnumWindowsProc(IntPtr window, IntPtr data);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr data);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    private static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromPoint(Point point, uint flags);

    [DllImport("shcore.dll")]
    private static extern int GetDpiForMonitor(
        IntPtr monitor, int dpiType, out uint dpiX, out uint dpiY
    );

    public static uint DpiAt(int x, int y)
    {
        var monitor = MonitorFromPoint(new Point { X = x, Y = y }, 2u);
        uint dpiX;
        uint dpiY;
        return GetDpiForMonitor(monitor, 0, out dpiX, out dpiY) == 0 ? dpiX : 96u;
    }

    public static uint ForegroundProcessId()
    {
        uint processId;
        GetWindowThreadProcessId(GetForegroundWindow(), out processId);
        return processId;
    }

    public static bool CloseProcess(uint processId)
    {
        var posted = false;
        EnumWindows((window, data) => {
            uint windowProcessId;
            GetWindowThreadProcessId(window, out windowProcessId);
            if (windowProcessId == processId && PostMessage(
                window, 0x0010u, IntPtr.Zero, IntPtr.Zero
            )) posted = true;
            return true;
        }, IntPtr.Zero);
        return posted;
    }
}
"@

function Get-ActivePowerPlan {
    $line = (& powercfg /getactivescheme 2>$null | Select-Object -First 1)
    if ($line) { return $line.Trim() }
    return "unavailable"
}

function Get-GradialGpuPercent {
    param([int]$TargetProcessId)

    try {
        $samples = (Get-Counter "\GPU Engine(*)\Utilization Percentage" -ErrorAction Stop).
            CounterSamples |
            Where-Object { $_.Path -match "pid_$($TargetProcessId)_" }
        if (-not $samples) {
            return $null
        }
        return [double](($samples | Measure-Object CookedValue -Sum).Sum)
    }
    catch {
        return $null
    }
}

function Measure-ColdLaunch {
    param(
        [int]$Count,
        [string]$AppPath,
        [string]$WorkingDirectory,
        [string]$LogPath
    )

    $measurements = @()
    for ($index = 0; $index -lt $Count; $index++) {
        if (Test-Path -LiteralPath $LogPath) {
            Remove-Item -LiteralPath $LogPath -Force
        }
        $stopwatch = [Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process -FilePath $AppPath `
            -WorkingDirectory $WorkingDirectory `
            -PassThru `
            -WindowStyle Hidden
        $healthy = $false
        $deadline = [DateTime]::UtcNow.AddSeconds(5)
        while ([DateTime]::UtcNow -lt $deadline -and -not $process.HasExited) {
            if (Test-Path -LiteralPath $LogPath) {
                $contents = Get-Content -Raw -LiteralPath $LogPath
                if ($contents -match "event_code=tray_health_changed .*health=Healthy") {
                    $healthy = $true
                    break
                }
            }
            Start-Sleep -Milliseconds 10
        }
        $stopwatch.Stop()
        if (-not $healthy) {
            if (-not $process.HasExited) {
                Stop-Process -Id $process.Id -Force
            }
            throw "Cold launch $($index + 1) did not become healthy within five seconds."
        }
        $measurements += $stopwatch.Elapsed.TotalMilliseconds
        [GradialPerformanceNative]::CloseProcess([uint32]$process.Id) | Out-Null
        if (-not $process.WaitForExit(5000)) {
            Stop-Process -Id $process.Id -Force
        }
    }
    return [double[]]$measurements
}

$powerPlanBefore = Get-ActivePowerPlan
$resourceSamples = @()
$foregroundStable = $false
$foregroundChanged = $false
$powerPlanChanged = $false
$campaignSucceeded = $false
try {
    New-Item -ItemType Directory -Path $gradialDirectory -Force | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $gradialDirectory "onboarding-v1.json"),
        '{"schema_version":1,"completed":true}',
        [System.Text.UTF8Encoding]::new($false)
    )
    $actionId = "67726164-6961-4c00-8000-000000000301"
    $profileId = "67726164-6961-4c00-8000-000000000003"
    $labels = @("N", "NE", "E", "SE", "S", "SW", "W", "NW")
    $slices = @($labels | ForEach-Object {
        [ordered]@{ label = $_; action_id = $actionId }
    })
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
            accent_color = [ordered]@{
                red = 124
                green = 154
                blue = 255
            }
            label_theme = "light"
            size_preset = "standard"
        }
        continuous_full_scale_radius_dip = 300
        global_profile = [ordered]@{
            id = $profileId
            name = "Performance fixture"
            menu = [ordered]@{ slice_count = 8; slices = $slices }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "10 ms performance delay"
                kind = [ordered]@{ type = "delay"; milliseconds = 10 }
            }
        )
    }
    [System.IO.File]::WriteAllText(
        (Join-Path $gradialDirectory "config.json"),
        ($configuration | ConvertTo-Json -Depth 10),
        [System.Text.UTF8Encoding]::new($false)
    )

    $env:LOCALAPPDATA = $temporaryRoot
    $smokeScript = Join-Path $PSScriptRoot "smoke-interaction.ps1"
    $childArguments = (
        "-NoProfile -ExecutionPolicy Bypass -File `"$smokeScript`" " +
        "-Configuration release -Geometry fixed-origin -Slices 8 -Direction E " +
        "-InvocationCount $($preset.invocation_count) -ExpectDelayExecution " +
        "-GestureDelayMilliseconds $($preset.gesture_delay_ms) " +
        "-InterInvocationDelayMilliseconds $($preset.inter_invocation_delay_ms) " +
        "-WarmupSeconds $($preset.warmup_seconds) " +
        "-PostInvocationIdleSeconds $($preset.idle_seconds) " +
        "-CaptureTiming -CaptureDurationSeconds $captureDuration " +
        "-MonitorIndex $MonitorIndex -ProcessIdFile `"$processIdFile`" " +
        "-PhaseFile `"$phaseFile`" " +
        "-ForegroundProcessFile `"$foregroundProcessFile`" " +
        "-TimingOutputDirectory `"$captureDirectory`""
    )
    if (-not $usePresentMon) {
        $childArguments += " -TimingMarkersOnly"
    }
    elseif ($PresentMonPath) {
        $childArguments += " -PresentMonPath `"$PresentMonPath`""
    }
    $child = Start-Process -FilePath "powershell.exe" `
        -ArgumentList $childArguments `
        -WorkingDirectory $workspace `
        -RedirectStandardOutput $stdoutPath `
        -RedirectStandardError $stderrPath `
        -PassThru `
        -WindowStyle Hidden

    $pidDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while (
        [DateTime]::UtcNow -lt $pidDeadline -and
        -not (Test-Path -LiteralPath $processIdFile) -and
        -not $child.HasExited
    ) {
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path -LiteralPath $processIdFile)) {
        throw "The performance interaction process did not publish Gradial's process ID."
    }
    $gradialProcessId = [int](Get-Content -Raw -LiteralPath $processIdFile)
    $previousTime = [DateTime]::UtcNow
    $previousCpu = 0.0
    $resourceSampleIndex = 0
    $gpuSampleInterval = [Math]::Max(
        1,
        [Math]::Ceiling(5000.0 / $preset.sample_interval_ms)
    )
    $powerSampleInterval = [Math]::Max(
        1,
        [Math]::Ceiling(30000.0 / $preset.sample_interval_ms)
    )
    while (-not $child.HasExited) {
        $process = Get-Process -Id $gradialProcessId -ErrorAction SilentlyContinue
        if ($process) {
            $now = [DateTime]::UtcNow
            $cpuMs = $process.TotalProcessorTime.TotalMilliseconds
            $wallMs = ($now - $previousTime).TotalMilliseconds
            $cpuPercent = if ($wallMs -gt 0 -and $previousCpu -gt 0) {
                (($cpuMs - $previousCpu) / $wallMs) * 100.0 /
                    [Environment]::ProcessorCount
            }
            else {
                $null
            }
            $phase = if (Test-Path -LiteralPath $phaseFile) {
                (Get-Content -Raw -LiteralPath $phaseFile).Trim()
            }
            else {
                "startup"
            }
            if (Test-Path -LiteralPath $foregroundProcessFile) {
                $expectedForeground = [uint32](
                    Get-Content -Raw -LiteralPath $foregroundProcessFile
                )
                $observedForeground =
                    [GradialPerformanceNative]::ForegroundProcessId()
                if (
                    $observedForeground -ne 0 -and
                    $observedForeground -ne $expectedForeground
                ) {
                    $foregroundChanged = $true
                }
            }
            if (
                $resourceSampleIndex % $powerSampleInterval -eq 0 -and
                (Get-ActivePowerPlan) -ne $powerPlanBefore
            ) {
                $powerPlanChanged = $true
            }
            $resourceSamples += [pscustomobject]@{
                captured_at = $now.ToString("o")
                phase = $phase
                working_set_mib = $process.WorkingSet64 / 1MB
                private_memory_mib = $process.PrivateMemorySize64 / 1MB
                cpu_percent = $cpuPercent
                gpu_percent = if ($resourceSampleIndex % $gpuSampleInterval -eq 0) {
                    Get-GradialGpuPercent $gradialProcessId
                }
                else {
                    $null
                }
            }
            $resourceSampleIndex++
            $previousTime = $now
            $previousCpu = $cpuMs
        }
        Start-Sleep -Milliseconds $preset.sample_interval_ms
        $child.Refresh()
    }
    $child.WaitForExit()
    $child.Refresh()
    $childExitCode = $child.ExitCode
    $childOutput = if (Test-Path -LiteralPath $stdoutPath) {
        Get-Content -Raw -LiteralPath $stdoutPath
    }
    else {
        ""
    }
    $childSucceeded = if ($null -ne $childExitCode) {
        $childExitCode -eq 0
    }
    else {
        $childOutput -match "smoke check passed"
    }
    if (-not $childSucceeded) {
        $failure = if (Test-Path -LiteralPath $stderrPath) {
            Get-Content -Raw -LiteralPath $stderrPath
        }
        else {
            ""
        }
        throw "The interaction campaign failed with code $childExitCode. $failure"
    }
    $foregroundStable = -not $foregroundChanged
    $resourceSamples | Export-Csv -LiteralPath $resourcePath -NoTypeInformation -Encoding utf8
    $interactionLogPath = Join-Path $reportDirectory "gradial-interaction.log"
    Copy-Item `
        -LiteralPath (Join-Path $gradialDirectory "logs\gradial_rCURRENT.log") `
        -Destination $interactionLogPath

    $coldLaunchValues = Measure-ColdLaunch `
        -Count $preset.cold_launch_samples `
        -AppPath $executable `
        -WorkingDirectory $workspace `
        -LogPath (Join-Path $gradialDirectory "logs\gradial_rCURRENT.log")
    $campaignSucceeded = $true
}
finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    if ($child -and -not $child.HasExited) {
        Stop-Process -Id $child.Id -Force
    }
    if (-not $campaignSucceeded) {
        $failedTemporary = Resolve-Path -LiteralPath $temporaryRoot -ErrorAction SilentlyContinue
        $systemTemporary = Resolve-Path -LiteralPath ([System.IO.Path]::GetTempPath())
        if (
            $failedTemporary -and
            $failedTemporary.Path.StartsWith(
                $systemTemporary.Path,
                [StringComparison]::OrdinalIgnoreCase
            ) -and
            (Split-Path -Leaf $failedTemporary.Path).StartsWith("gradial-performance-")
        ) {
            Remove-Item -LiteralPath $failedTemporary.Path -Recurse -Force
        }
    }
}
$powerPlanAfter = Get-ActivePowerPlan
$powerPlanStable = -not $powerPlanChanged -and $powerPlanBefore -eq $powerPlanAfter
$validProtocol = $gateEligible -and $foregroundStable -and $powerPlanStable

$markerCsv = Get-ChildItem -LiteralPath $captureDirectory -Filter "gradial-markers-*.csv" |
    Sort-Object LastWriteTime |
    Select-Object -Last 1
$presentCsv = Get-ChildItem -LiteralPath $captureDirectory -Filter "gradial-presents-*.csv" |
    Sort-Object LastWriteTime |
    Select-Object -Last 1
$markers = Read-TimingMarkers $markerCsv.FullName
$qpcFrequency = (
    $markers | Select-Object -First 1 -ExpandProperty qpc_frequency -ErrorAction SilentlyContinue
)
$presents = Get-PresentEvents $presentCsv.FullName $qpcFrequency
$logPath = Join-Path $reportDirectory "gradial-interaction.log"
$log = if (Test-Path -LiteralPath $logPath) {
    Get-Content -Raw -LiteralPath $logPath
}
else {
    ""
}

$metrics = [Collections.ArrayList]::new()
$openCheckpoint = Get-MarkerDurations `
    -Markers $markers `
    -StartMarker "activation_dequeued" `
    -StartField source_qpc `
    -EndMarker "overlay_shown" `
    -EndField marker_qpc
$openCheckpointStats = Get-Statistics $openCheckpoint
[void]$metrics.Add((New-Metric `
    -Name "button_down_to_application_frame_checkpoint" `
    -Unit "ms" `
    -MeasurementStatus $(if ($openCheckpointStats) { "measured" } else { "unavailable" }) `
    -Statistics $openCheckpointStats `
    -Evidence "Gradial ETW activation_dequeued to overlay_shown" `
    -Reason "Diagnostic only; an application checkpoint is not a displayed frame."))

$highlightCheckpoint = Get-MarkerDurations `
    -Markers $markers `
    -StartMarker "highlight_requested" `
    -StartField source_qpc `
    -EndMarker "highlight_applied" `
    -EndField marker_qpc
$highlightCheckpointStats = Get-Statistics $highlightCheckpoint
[void]$metrics.Add((New-Metric `
    -Name "pointer_to_application_highlight_checkpoint" `
    -Unit "ms" `
    -MeasurementStatus $(if ($highlightCheckpointStats) { "measured" } else { "unavailable" }) `
    -Statistics $highlightCheckpointStats `
    -Evidence "Gradial ETW highlight_requested to highlight_applied" `
    -Reason "Diagnostic only; an application checkpoint is not a displayed highlight."))

$displayOpen = Get-DisplayedDurations `
    -Markers $markers `
    -PresentEvents $presents `
    -ReadyMarker "overlay_shown" `
    -InputMarker "activation_dequeued"
$displayOpenStats = Get-Statistics $displayOpen
$displayOpenGate = Get-GateResult `
    -Statistics $displayOpenStats `
    -ProtocolEligible $validProtocol `
    -P95Limit 25 `
    -P99Limit 40
[void]$metrics.Add((New-Metric `
    -Name "button_down_to_displayed_usable_frame" `
    -Unit "ms" `
    -MeasurementStatus $(if ($displayOpenStats) { "measured" } else { "unavailable" }) `
    -GateResult $displayOpenGate `
    -Statistics $displayOpenStats `
    -Evidence "Gradial ETW correlated with PresentMon displayed events" `
    -Reason $(if (-not $usePresentMon) {
        "PresentMon display capture was unavailable without an elevated session."
    } elseif (-not $gateEligible) {
        "Development protocol is not eligible for a release gate."
    } else { "" })))

$displayHighlight = Get-DisplayedDurations `
    -Markers $markers `
    -PresentEvents $presents `
    -ReadyMarker "highlight_applied" `
    -InputMarker "highlight_requested"
$displayHighlightStats = Get-Statistics $displayHighlight
$refreshRate = (
    Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue |
    Where-Object CurrentRefreshRate |
    Select-Object -First 1 -ExpandProperty CurrentRefreshRate
)
if (-not $refreshRate) { $refreshRate = 60 }
$refreshPeriodMs = 1000.0 / [double]$refreshRate
$displayHighlightGate = Get-GateResult `
    -Statistics $displayHighlightStats `
    -ProtocolEligible $validProtocol `
    -P95Limit ($refreshPeriodMs + 4) `
    -P99Limit (2 * $refreshPeriodMs) `
    -P95Inclusive `
    -P99Inclusive
[void]$metrics.Add((New-Metric `
    -Name "pointer_to_displayed_highlight" `
    -Unit "ms" `
    -MeasurementStatus $(if ($displayHighlightStats) { "measured" } else { "unavailable" }) `
    -GateResult $displayHighlightGate `
    -Statistics $displayHighlightStats `
    -Evidence "Gradial ETW correlated with PresentMon displayed events" `
    -Reason $(if (-not $usePresentMon) {
        "PresentMon display capture was unavailable without an elevated session."
    } elseif (-not $gateEligible) {
        "Development protocol is not eligible for a release gate."
    } else { "" })))

$dispatch = Get-MarkerDurations `
    -Markers $markers `
    -StartMarker "release_dequeued" `
    -StartField source_qpc `
    -EndMarker "action_dispatched" `
    -EndField marker_qpc
$dispatchStats = Get-Statistics $dispatch
$dispatchGate = Get-GateResult `
    -Statistics $dispatchStats `
    -ProtocolEligible $validProtocol `
    -P95Limit 10 `
    -P99Limit 20
[void]$metrics.Add((New-Metric `
    -Name "release_to_executor_dispatch" `
    -Unit "ms" `
    -MeasurementStatus $(if ($dispatchStats) { "measured" } else { "unavailable" }) `
    -GateResult $dispatchGate `
    -Statistics $dispatchStats `
    -Evidence "Gradial ETW release_dequeued to action_dispatched" `
    -Reason $(if (-not $gateEligible) {
        "Development protocol is not eligible for a release gate."
    } else { "" })))

[void]$metrics.Add((New-Metric `
    -Name "release_to_completed_sendinput_shortcut" `
    -Unit "ms" `
    -MeasurementStatus "not_run" `
    -Reason "The safe Delay fixture does not synthesize keyboard input; this gate needs an isolated shortcut receiver."))

$hookMatch = [regex]::Matches(
    $log,
    "event_code=hook_thread_stopped .*callback_count=(?<count>[0-9]+) " +
    "p99_callback_us=(?<p99>[0-9]+) maximum_callback_us=(?<max>[0-9]+)"
) | Select-Object -Last 1
$hookStats = $null
$hookGate = "not_evaluated"
if ($hookMatch) {
    $hookStats = [ordered]@{
        sample_count = [int64]$hookMatch.Groups["count"].Value
        p50 = $null
        p95 = $null
        p99 = [double]$hookMatch.Groups["p99"].Value
        max = [double]$hookMatch.Groups["max"].Value
    }
    if ($validProtocol) {
        $hookGate = if ($hookStats.p99 -lt 250 -and $hookStats.max -lt 1000) {
            "pass"
        }
        else {
            "fail"
        }
    }
}
[void]$metrics.Add((New-Metric `
    -Name "hook_callback_duration" `
    -Unit "us" `
    -MeasurementStatus $(if ($hookStats) { "measured" } else { "unavailable" }) `
    -GateResult $hookGate `
    -Statistics $hookStats `
    -Evidence "Fixed allocation-free in-process hook histogram" `
    -Reason $(if (-not $gateEligible) {
        "Development protocol is not eligible for a release gate."
    } else { "" })))

$coldStats = Get-Statistics $coldLaunchValues
$coldGate = Get-GateResult `
    -Statistics $coldStats `
    -ProtocolEligible $validProtocol `
    -P95Limit 750 `
    -P99Limit ([double]::MaxValue)
[void]$metrics.Add((New-Metric `
    -Name "cold_launch_to_healthy" `
    -Unit "ms" `
    -MeasurementStatus "measured" `
    -GateResult $coldGate `
    -Statistics $coldStats `
    -Evidence "Process start to logged Healthy tray state" `
    -Reason $(if (-not $gateEligible) {
        "Development protocol is not eligible for a release gate."
    } else { "" })))

$idleSamples = @($resourceSamples | Where-Object phase -eq "idle")
$idleCpu = [double[]]@(
    $idleSamples |
    Select-Object -Skip 1 |
    Where-Object { $null -ne $_.cpu_percent } |
    ForEach-Object { [double]$_.cpu_percent }
)
$idleGpu = [double[]]@(
    $idleSamples |
    Where-Object { $null -ne $_.gpu_percent } |
    ForEach-Object { [double]$_.gpu_percent }
)
$idleWorkingSet = [double[]]@(
    $idleSamples | ForEach-Object { [double]$_.working_set_mib }
)
$cpuStats = Get-Statistics $idleCpu
$gpuStats = Get-Statistics $idleGpu
$workingSetStats = Get-Statistics $idleWorkingSet
$resourceEligible = $validProtocol -and $preset.idle_seconds -ge 600
$cpuAverage = if ($idleCpu.Count -gt 0) {
    ($idleCpu | Measure-Object -Average).Average
}
else { $null }
$gpuAverage = if ($idleGpu.Count -gt 0) {
    ($idleGpu | Measure-Object -Average).Average
}
else { $null }
$cpuGate = if ($resourceEligible -and $null -ne $cpuAverage) {
    if ($cpuAverage -lt 0.2) { "pass" } else { "fail" }
}
else { "not_evaluated" }
$gpuGate = if ($resourceEligible -and $null -ne $gpuAverage) {
    if ($gpuAverage -lt 0.5) { "pass" } else { "fail" }
}
else { "not_evaluated" }
$workingSetGate = if (
    $validProtocol -and
    $preset.idle_seconds -ge 1800 -and
    $workingSetStats
) {
    if ($workingSetStats.max -lt 80) { "pass" } else { "fail" }
}
else { "not_evaluated" }
[void]$metrics.Add((New-Metric `
    -Name "warm_idle_cpu" `
    -Unit "percent" `
    -MeasurementStatus $(if ($cpuStats) { "measured" } else { "unavailable" }) `
    -GateResult $cpuGate `
    -Statistics $cpuStats `
    -Evidence "Process CPU samples; arithmetic mean is $cpuAverage" `
    -Reason $(if (-not $resourceEligible) {
        "Release evaluation requires at least ten minutes of warm idle."
    } else { "" })))
[void]$metrics.Add((New-Metric `
    -Name "warm_idle_gpu" `
    -Unit "percent" `
    -MeasurementStatus $(if ($gpuStats) { "measured" } else { "unavailable" }) `
    -GateResult $gpuGate `
    -Statistics $gpuStats `
    -Evidence "Per-process Windows GPU Engine counters; arithmetic mean is $gpuAverage" `
    -Reason $(if (-not $resourceEligible) {
        "Release evaluation requires at least ten minutes of warm idle."
    } else { "" })))
[void]$metrics.Add((New-Metric `
    -Name "working_set_after_campaign" `
    -Unit "MiB" `
    -MeasurementStatus $(if ($workingSetStats) { "measured" } else { "unavailable" }) `
    -GateResult $workingSetGate `
    -Statistics $workingSetStats `
    -Evidence "Working set samples during the post-invocation idle phase" `
    -Reason $(if ($workingSetGate -eq "not_evaluated") {
        "Release evaluation requires 1,000 invocations and thirty minutes idle."
    } else { "" })))

$queueOverflows = ([regex]::Matches($log, "event_code=input_queue_overflow")).Count
$queueStats = [ordered]@{
    sample_count = $preset.invocation_count
    p50 = $queueOverflows
    p95 = $queueOverflows
    p99 = $queueOverflows
    max = $queueOverflows
}
$queueGate = if ($validProtocol) {
    if ($queueOverflows -eq 0) { "pass" } else { "fail" }
}
else { "not_evaluated" }
[void]$metrics.Add((New-Metric `
    -Name "input_queue_overflow_count" `
    -Unit "count" `
    -MeasurementStatus "measured" `
    -GateResult $queueGate `
    -Statistics $queueStats `
    -Evidence "Structured application log" `
    -Reason $(if (-not $gateEligible) {
        "Development protocol is not eligible for a release gate."
    } else { "" })))

$cpu = Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue |
    Select-Object -First 1 -ExpandProperty Name
$gpus = @(
    Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue |
    Select-Object -ExpandProperty Name
)
$windows = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue
$displays = @(
    [System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
        $centerX = $_.Bounds.X + [int]($_.Bounds.Width / 2)
        $centerY = $_.Bounds.Y + [int]($_.Bounds.Height / 2)
        $dpi = [GradialPerformanceNative]::DpiAt($centerX, $centerY)
        "$($_.DeviceName) $($_.Bounds.Width)x$($_.Bounds.Height) " +
        "at $($_.Bounds.X),$($_.Bounds.Y) primary=$($_.Primary) " +
        "dpi=$dpi refresh=${refreshRate}Hz"
    }
)
$gateResults = @($metrics | ForEach-Object { $_.gate_result })
$overall = if ($Mode -eq "Release" -and -not ($foregroundStable -and $powerPlanStable)) {
    "invalid"
}
elseif ($gateResults -contains "fail") {
    "fail"
}
elseif ($gateResults -contains "not_evaluated") {
    "not_evaluated"
}
else {
    "pass"
}
$notes = @(
    "Application frame checkpoints are diagnostic and never substitute for displayed-frame evidence.",
    "The interaction campaign used a harmless 10 ms Delay action on every slice.",
    "The shortcut-completion gate remains intentionally not run until an isolated receiver is used.",
    "Raw ETW, PresentMon, resource, stdout, and stderr evidence remains beside this report."
)
if (-not $isElevated) {
    $notes += "This PowerShell session was not elevated, so PresentMon display evidence was not captured."
}
if ($Mode -eq "Development") {
    $notes += "Development mode is a harness check and local baseline, not a release qualification."
}
$report = [ordered]@{
    report_schema_version = 1
    captured_at = (Get-Date).ToUniversalTime().ToString("o")
    overall_gate_result = $overall
    protocol = [ordered]@{
        mode = $Mode
        gate_eligible = $gateEligible
        invocation_count = $preset.invocation_count
        warmup_seconds = $preset.warmup_seconds
        idle_seconds = $preset.idle_seconds
        cold_launch_samples = $preset.cold_launch_samples
        detached_debugger = $true
    }
    capabilities = [ordered]@{
        elevated = $isElevated
        presentmon = if ($usePresentMon) { "captured" } elseif ($presentMonAvailable) {
            "installed_but_elevation_required"
        } else {
            "not_installed"
        }
        timing_markers = if ($markers.Count -gt 0) { "captured" } else { "unavailable" }
        gpu_counters = if ($gpuStats) { "captured" } else { "unavailable" }
        windows_performance_toolkit = if (
            Test-Path -LiteralPath (
                "C:\Program Files (x86)\Windows Kits\10\Windows Performance Toolkit"
            )
        ) { "installed" } else { "not_found" }
    }
    validity = [ordered]@{
        foreground_stable = $foregroundStable
        power_plan_stable = $powerPlanStable
        valid = $foregroundStable -and $powerPlanStable
    }
    machine = [ordered]@{
        windows = if ($windows) {
            "$($windows.Caption) $($windows.Version) build $($windows.BuildNumber)"
        } else { "unavailable" }
        cpu = if ($cpu) { $cpu.Trim() } else { "unavailable" }
        gpu = if ($gpus) { $gpus -join "; " } else { "unavailable" }
        logical_processors = [Environment]::ProcessorCount
        displays = $displays
        dpi = "recorded per display in the displays field"
        refresh_hz = $refreshRate
        power_plan = $powerPlanBefore
    }
    metrics = @($metrics)
    notes = $notes
}
$paths = Write-ReportFiles -Report $report -Directory $reportDirectory

$resolvedTemporary = Resolve-Path -LiteralPath $temporaryRoot -ErrorAction SilentlyContinue
$resolvedSystemTemporary = Resolve-Path -LiteralPath ([System.IO.Path]::GetTempPath())
if (
    $resolvedTemporary -and
    $resolvedTemporary.Path.StartsWith(
        $resolvedSystemTemporary.Path,
        [StringComparison]::OrdinalIgnoreCase
    ) -and
    (Split-Path -Leaf $resolvedTemporary.Path).StartsWith("gradial-performance-")
) {
    Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
}

Write-Host "Gradial $Mode performance report complete."
Write-Host "  Markdown: $($paths.markdown)"
Write-Host "  JSON:     $($paths.json)"
Write-Host "  CSV:      $($paths.csv)"
Write-Host "  Overall:  $overall"
