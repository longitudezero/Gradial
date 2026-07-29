param(
    [ValidateSet("quick", "standard", "extended")]
    [string]$Suite = "standard",

    [ValidateSet("debug", "release")]
    [string]$Configuration = "debug",

    [ValidateRange(0, 100)]
    [int]$CornerTrialsPerDirection = 0,

    [ValidateRange(0, 1000)]
    [int]$RapidInvocationCount = 0,

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\captures\interaction-matrix")
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class GradialMatrixDisplay
{
    [StructLayout(LayoutKind.Sequential)]
    public struct Point
    {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromPoint(Point point, uint flags);

    [DllImport("shcore.dll")]
    private static extern int GetDpiForMonitor(
        IntPtr monitor,
        int dpiType,
        out uint dpiX,
        out uint dpiY
    );

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
}
"@

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$smokeScript = Join-Path $PSScriptRoot "smoke-interaction.ps1"
$executable = Join-Path $workspace "target\$Configuration\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's $Configuration configuration before running the interaction matrix."
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the isolated interaction matrix."
}

if ($CornerTrialsPerDirection -eq 0) {
    $CornerTrialsPerDirection = if ($Suite -eq "extended") { 10 } else { 1 }
}
if ($RapidInvocationCount -eq 0) {
    $RapidInvocationCount = switch ($Suite) {
        "quick" { 10 }
        "standard" { 25 }
        "extended" { 100 }
    }
}

$runId = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$runDirectory = Join-Path $OutputDirectory $runId
New-Item -ItemType Directory -Path $runDirectory -Force | Out-Null
$startedUtc = [DateTimeOffset]::UtcNow

function Read-CimValue {
    param(
        [string]$ClassName,
        [string[]]$Properties
    )

    try {
        @(Get-CimInstance -ClassName $ClassName -ErrorAction Stop |
            Select-Object -Property $Properties)
    }
    catch {
        @()
    }
}

$screens = [System.Windows.Forms.Screen]::AllScreens
$monitors = for ($index = 0; $index -lt $screens.Count; $index++) {
    $screen = $screens[$index]
    $centerX = $screen.Bounds.Left + [int]($screen.Bounds.Width / 2)
    $centerY = $screen.Bounds.Top + [int]($screen.Bounds.Height / 2)
    [pscustomobject]@{
        index = $index
        device_name = $screen.DeviceName
        primary = $screen.Primary
        dpi = [GradialMatrixDisplay]::EffectiveDpiAt($centerX, $centerY)
        bounds = [pscustomobject]@{
            left = $screen.Bounds.Left
            top = $screen.Bounds.Top
            right = $screen.Bounds.Right
            bottom = $screen.Bounds.Bottom
            width = $screen.Bounds.Width
            height = $screen.Bounds.Height
        }
        work_area = [pscustomobject]@{
            left = $screen.WorkingArea.Left
            top = $screen.WorkingArea.Top
            right = $screen.WorkingArea.Right
            bottom = $screen.WorkingArea.Bottom
            width = $screen.WorkingArea.Width
            height = $screen.WorkingArea.Height
        }
    }
}

$os = Read-CimValue -ClassName "Win32_OperatingSystem" -Properties @(
    "Caption",
    "Version",
    "BuildNumber"
)
$cpu = Read-CimValue -ClassName "Win32_Processor" -Properties @("Name")
$video = Read-CimValue -ClassName "Win32_VideoController" -Properties @(
    "Name",
    "DriverVersion",
    "CurrentRefreshRate"
)

$scenarioResults = [System.Collections.Generic.List[object]]::new()

function Invoke-MatrixScenario {
    param(
        [string]$Id,
        [string]$Category,
        [string[]]$SmokeArguments,
        [int]$ExpectedInvocations,
        [Nullable[int]]$MonitorIndex = $null,
        [string]$Notes = ""
    )

    $scenarioStarted = [DateTimeOffset]::UtcNow
    Write-Host "[$($scenarioResults.Count + 1)] $Id"
    $previousErrorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass `
            -File $smokeScript @SmokeArguments 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorPreference
    }
    $scenarioEnded = [DateTimeOffset]::UtcNow
    $status = if ($exitCode -eq 0) { "passed" } else { "failed" }
    $monitor = if ($null -ne $MonitorIndex) {
        $monitors | Where-Object index -eq $MonitorIndex | Select-Object -First 1
    } else {
        $null
    }
    $record = [pscustomobject]@{
        id = $Id
        category = $Category
        status = $status
        started_utc = $scenarioStarted.ToString("O")
        duration_ms = [Math]::Round(($scenarioEnded - $scenarioStarted).TotalMilliseconds, 3)
        expected_invocations = $ExpectedInvocations
        monitor_index = if ($monitor) { $monitor.index } else { $null }
        monitor_dpi = if ($monitor) { $monitor.dpi } else { $null }
        arguments = $SmokeArguments -join " "
        notes = $Notes
        output = (@($output) -join [Environment]::NewLine).Trim()
        exit_code = $exitCode
    }
    $scenarioResults.Add($record)
    if ($status -eq "failed") {
        Write-Warning "$Id failed: $($record.output)"
    }
}

$base = @("-Configuration", $Configuration)
Invoke-MatrixScenario `
    -Id "center-eight-directions" `
    -Category "gesture" `
    -SmokeArguments ($base + @(
        "-Geometry", "fixed-origin",
        "-Slices", "8",
        "-ActivationPoint", "center",
        "-AllDirections",
        "-GestureDelayMilliseconds", "25",
        "-InterInvocationDelayMilliseconds", "75"
    )) `
    -ExpectedInvocations 8 `
    -MonitorIndex 0

if ($Suite -ne "quick" -and $Configuration -eq "debug") {
    Invoke-MatrixScenario `
        -Id "center-four-directions" `
        -Category "gesture" `
        -SmokeArguments ($base + @(
            "-Geometry", "fixed-origin",
            "-Slices", "4",
            "-ActivationPoint", "center",
            "-AllDirections",
            "-GestureDelayMilliseconds", "25",
            "-InterInvocationDelayMilliseconds", "75"
        )) `
        -ExpectedInvocations 4 `
        -MonitorIndex 0
}

$cornerPoints = if ($Suite -eq "quick") {
    @("top-left")
} else {
    @("top-left", "top-right", "bottom-right", "bottom-left")
}
foreach ($point in $cornerPoints) {
    Invoke-MatrixScenario `
        -Id "corner-$point-eight-directions" `
        -Category "edge" `
        -SmokeArguments ($base + @(
            "-Geometry", "auto",
            "-Slices", "8",
            "-ActivationPoint", $point,
            "-AllDirections",
            "-InvocationCount", $CornerTrialsPerDirection,
            "-GestureDelayMilliseconds", "25",
            "-InterInvocationDelayMilliseconds", "75"
        )) `
        -ExpectedInvocations (8 * $CornerTrialsPerDirection) `
        -MonitorIndex 0
}

if ($Suite -ne "quick") {
    foreach ($point in @("top-edge", "right-edge", "bottom-edge", "left-edge")) {
        Invoke-MatrixScenario `
            -Id "edge-$point-eight-directions" `
            -Category "edge" `
            -SmokeArguments ($base + @(
                "-Geometry", "auto",
                "-Slices", "8",
                "-ActivationPoint", $point,
                "-AllDirections",
                "-GestureDelayMilliseconds", "25",
                "-InterInvocationDelayMilliseconds", "75"
            )) `
            -ExpectedInvocations 8 `
            -MonitorIndex 0
    }
}

Invoke-MatrixScenario `
    -Id "rapid-overlapping-invocations" `
    -Category "rapid" `
    -SmokeArguments ($base + @(
        "-Geometry", "fixed-origin",
        "-Slices", "8",
        "-ActivationPoint", "center",
        "-Direction", "E",
        "-InvocationCount", $RapidInvocationCount,
        "-QuickFlick",
        "-InterInvocationDelayMilliseconds", "10"
    )) `
    -ExpectedInvocations $RapidInvocationCount `
    -MonitorIndex 0

foreach ($monitor in $monitors) {
    Invoke-MatrixScenario `
        -Id "monitor-$($monitor.index)-dpi-$($monitor.dpi)" `
        -Category "display" `
        -SmokeArguments ($base + @(
            "-Geometry", "auto",
            "-Slices", "8",
            "-ActivationPoint", "center",
            "-Direction", "E",
            "-MonitorIndex", $monitor.index
        )) `
        -ExpectedInvocations 1 `
        -MonitorIndex $monitor.index
}

Invoke-MatrixScenario `
    -Id "display-topology-notification" `
    -Category "display" `
    -SmokeArguments ($base + @(
        "-Geometry", "fixed-origin",
        "-SimulateDisplayChange"
    )) `
    -ExpectedInvocations 1 `
    -MonitorIndex 0 `
    -Notes "Synthetic WM_DISPLAYCHANGE exercises cancellation and pool replacement; it does not change physical DPI."

Invoke-MatrixScenario `
    -Id "resume-reregistration" `
    -Category "recovery" `
    -SmokeArguments ($base + @(
        "-Geometry", "fixed-origin",
        "-SimulateInputReregistration"
    )) `
    -ExpectedInvocations 1 `
    -MonitorIndex 0

if ($Configuration -eq "debug") {
    Invoke-MatrixScenario `
        -Id "heartbeat-auto-recovery" `
        -Category "recovery" `
        -SmokeArguments ($base + @(
            "-Geometry", "fixed-origin",
            "-SimulateInputFault", "heartbeat"
        )) `
        -ExpectedInvocations 1 `
        -MonitorIndex 0
    Invoke-MatrixScenario `
        -Id "queue-overflow-manual-retry" `
        -Category "recovery" `
        -SmokeArguments ($base + @(
            "-Geometry", "fixed-origin",
            "-SimulateInputFault", "queue-overflow"
        )) `
        -ExpectedInvocations 1 `
        -MonitorIndex 0
}

function Scenario-Status {
    param([string]$Id)
    ($scenarioResults | Where-Object id -eq $Id | Select-Object -First 1).status
}

function Combined-Status {
    param([string[]]$Ids)
    $records = @($scenarioResults | Where-Object { $_.id -in $Ids })
    if ($records.Count -eq 0) { return "not_run" }
    if ($records.status -contains "failed") { return "failed" }
    "passed"
}

$cornerIds = @($scenarioResults |
    Where-Object category -eq "edge" |
    Where-Object id -like "corner-*" |
    ForEach-Object id)
$edgeIds = @($scenarioResults |
    Where-Object category -eq "edge" |
    Where-Object id -like "edge-*" |
    ForEach-Object id)
$actualCornerTrials = $cornerPoints.Count * 8 * $CornerTrialsPerDirection
$totalExpectedInvocations = (
    $scenarioResults | Measure-Object -Property expected_invocations -Sum
).Sum
$dpiValues = @($monitors | ForEach-Object dpi | Sort-Object -Unique)
$hasNegativeMonitor = @($monitors | Where-Object {
    $_.bounds.left -lt 0 -or $_.bounds.top -lt 0
}).Count -gt 0
$refreshRates = @($video |
    ForEach-Object CurrentRefreshRate |
    Where-Object { $null -ne $_ -and $_ -gt 0 } |
    Sort-Object -Unique)

$coverage = [System.Collections.Generic.List[object]]::new()
$coverage.Add([pscustomobject]@{
    requirement = "four/eight center directions"
    status = if ($Suite -eq "quick") {
        if ((Scenario-Status "center-eight-directions") -eq "failed") {
            "failed"
        } else {
            "partial"
        }
    } else {
        Combined-Status @("center-eight-directions", "center-four-directions")
    }
    evidence = if ($Suite -eq "quick") {
        "Eight slices exercised; four-slice scenario not included in quick suite."
    } else {
        "All four and eight center directions exercised."
    }
})
$coverage.Add([pscustomobject]@{
    requirement = "every direction from every corner"
    status = if ($Suite -eq "quick") {
        if ((Combined-Status $cornerIds) -eq "failed") { "failed" } else { "partial" }
    } else {
        Combined-Status $cornerIds
    }
    evidence = "$actualCornerTrials synthetic corner trials across $($cornerPoints.Count) corner(s)."
})
$coverage.Add([pscustomobject]@{
    requirement = "320 automated corner-direction repetitions"
    status = if ($actualCornerTrials -ge 320 -and (Combined-Status $cornerIds) -eq "passed") {
        "passed"
    } else {
        "not_run"
    }
    evidence = "$actualCornerTrials of 320 automated repetitions executed; human direction error and selection time are not inferred."
})
$coverage.Add([pscustomobject]@{
    requirement = "every edge midpoint"
    status = if ($Suite -eq "quick") { "not_run" } else { Combined-Status $edgeIds }
    evidence = if ($Suite -eq "quick") {
        "Excluded from quick suite."
    } else {
        "All eight directions exercised at four edge midpoints."
    }
})
$coverage.Add([pscustomobject]@{
    requirement = "rapid invocation cleanup"
    status = Scenario-Status "rapid-overlapping-invocations"
    evidence = "$RapidInvocationCount quick flicks with 10 ms inter-invocation delay."
})
$coverage.Add([pscustomobject]@{
    requirement = "focus preservation"
    status = if ($scenarioResults.status -contains "failed") { "failed" } else { "passed" }
    evidence = "$totalExpectedInvocations expected selected invocations checked at activation and after close; any rejected checkpoint fails its scenario."
})
$coverage.Add([pscustomobject]@{
    requirement = "available monitor and DPI routing"
    status = Combined-Status @($scenarioResults |
        Where-Object category -eq "display" |
        Where-Object id -like "monitor-*" |
        ForEach-Object id)
    evidence = "$($monitors.Count) monitor(s) exercised; observed effective DPI values: $($dpiValues -join ', ')."
})
$coverage.Add([pscustomobject]@{
    requirement = "mixed 100%/150% DPI pair"
    status = if (96 -in $dpiValues -and 144 -in $dpiValues) { "available" } else { "unavailable" }
    evidence = "Observed effective DPI values: $($dpiValues -join ', ')."
})
$coverage.Add([pscustomobject]@{
    requirement = "mixed 125%/200% DPI pair"
    status = if (120 -in $dpiValues -and 192 -in $dpiValues) { "available" } else { "unavailable" }
    evidence = "Observed effective DPI values: $($dpiValues -join ', ')."
})
$coverage.Add([pscustomobject]@{
    requirement = "negative-coordinate monitor"
    status = if ($hasNegativeMonitor) { "available" } else { "unavailable" }
    evidence = if ($hasNegativeMonitor) {
        "At least one monitor begins left of or above the primary origin."
    } else {
        "No active monitor has negative left/top coordinates."
    }
})
$coverage.Add([pscustomobject]@{
    requirement = "120 Hz or faster display"
    status = if (@($refreshRates | Where-Object { $_ -ge 120 }).Count -gt 0) {
        "available"
    } else {
        "unavailable"
    }
    evidence = "Video-controller refresh observations: $($refreshRates -join ', ') Hz; controller data is not mapped to individual monitors."
})
$coverage.Add([pscustomobject]@{
    requirement = "display topology cancellation and rewarm"
    status = Scenario-Status "display-topology-notification"
    evidence = "Synthetic display-change notification; physical plug/unplug and DPI changes remain hardware tests."
})
$recoveryIds = @("resume-reregistration")
if ($Configuration -eq "debug") {
    $recoveryIds += @("heartbeat-auto-recovery", "queue-overflow-manual-retry")
}
$coverage.Add([pscustomobject]@{
    requirement = "input recovery paths"
    status = Combined-Status $recoveryIds
    evidence = if ($Configuration -eq "debug") {
        "Resume re-registration, heartbeat auto-recovery, and queue-overflow manual retry exercised."
    } else {
        "Resume re-registration exercised; debug fault injection is absent from release builds."
    }
})
foreach ($requirement in @(
    "physical DPI change",
    "physical monitor plug/unplug",
    "primary-monitor change",
    "physical sleep/resume",
    "session lock/unlock",
    "Explorer restart",
    "1,000-invocation presented-frame latency run",
    "10,000-invocation focus campaign",
    "human direction-error and selection-time study"
)) {
    $coverage.Add([pscustomobject]@{
        requirement = $requirement
        status = "not_run"
        evidence = "Requires physical state change, extended performance capture, or human participation."
    })
}

$endedUtc = [DateTimeOffset]::UtcNow
$summary = [pscustomobject]@{
    scenarios = $scenarioResults.Count
    passed = @($scenarioResults | Where-Object status -eq "passed").Count
    failed = @($scenarioResults | Where-Object status -eq "failed").Count
    expected_invocations = $totalExpectedInvocations
    coverage_passed = @($coverage | Where-Object status -eq "passed").Count
    coverage_partial = @($coverage | Where-Object status -eq "partial").Count
    coverage_available = @($coverage | Where-Object status -eq "available").Count
    coverage_unavailable = @($coverage | Where-Object status -eq "unavailable").Count
    coverage_not_run = @($coverage | Where-Object status -eq "not_run").Count
}
$report = [ordered]@{
    schema_version = 1
    run_id = $runId
    suite = $Suite
    configuration = $Configuration
    started_utc = $startedUtc.ToString("O")
    ended_utc = $endedUtc.ToString("O")
    duration_ms = [Math]::Round(($endedUtc - $startedUtc).TotalMilliseconds, 3)
    requested = [ordered]@{
        corner_trials_per_direction = $CornerTrialsPerDirection
        rapid_invocation_count = $RapidInvocationCount
    }
    host = [ordered]@{
        computer_name = $env:COMPUTERNAME
        operating_system = $os
        cpu = $cpu
        video = $video
        monitors = $monitors
    }
    summary = $summary
    scenarios = $scenarioResults
    coverage = $coverage
}

$jsonPath = Join-Path $runDirectory "interaction-matrix.json"
$scenarioCsvPath = Join-Path $runDirectory "scenarios.csv"
$coverageCsvPath = Join-Path $runDirectory "coverage.csv"
$report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $jsonPath -Encoding UTF8
$scenarioResults | Select-Object `
    id,
    category,
    status,
    started_utc,
    duration_ms,
    expected_invocations,
    monitor_index,
    monitor_dpi,
    arguments,
    notes,
    exit_code,
    output |
    Export-Csv -LiteralPath $scenarioCsvPath -NoTypeInformation -Encoding UTF8
$coverage | Export-Csv -LiteralPath $coverageCsvPath -NoTypeInformation -Encoding UTF8

Write-Host ""
Write-Host "Interaction matrix complete: $($summary.passed) passed, $($summary.failed) failed."
Write-Host "Expected selected invocations: $($summary.expected_invocations)"
Write-Host "JSON report:     $jsonPath"
Write-Host "Scenario table:  $scenarioCsvPath"
Write-Host "Coverage table:  $coverageCsvPath"

if ($summary.failed -gt 0) {
    exit 1
}
