param(
    [ValidateSet("Development", "Release")]
    [string]$Mode = "Development",

    [ValidateSet("Both", "debug", "release")]
    [string]$Build = "Both",

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\captures\plugin-performance"),

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
        mean = ($Values | Measure-Object -Average).Average
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

    if (-not $Path -or -not (Test-Path -LiteralPath $Path) -or $QpcFrequency -le 0) {
        return @()
    }
    $rows = @(Import-Csv -LiteralPath $Path)
    if ($rows.Count -eq 0) {
        return @()
    }
    $names = @($rows[0].PSObject.Properties.Name)
    $qpcColumn = @("CPUStartQPC", "CPUStartQpc") |
        Where-Object { $names -contains $_ } | Select-Object -First 1
    $displayColumn = @("msUntilDisplayed", "MsUntilDisplayed") |
        Where-Object { $names -contains $_ } | Select-Object -First 1
    if (-not $qpcColumn -or -not $displayColumn) {
        return @()
    }
    $events = @()
    foreach ($row in $rows) {
        [int64]$start = 0
        [double]$displayMilliseconds = 0
        if (
            [int64]::TryParse([string]$row.$qpcColumn, [ref]$start) -and
            [double]::TryParse(
                [string]$row.$displayColumn,
                [Globalization.NumberStyles]::Float,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$displayMilliseconds
            ) -and
            $displayMilliseconds -ge 0
        ) {
            $events += [pscustomobject]@{
                start_qpc = $start
                displayed_qpc = $start + [int64][Math]::Round(
                    $displayMilliseconds * $QpcFrequency / 1000.0
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

function Get-PluginLifecycleSamples {
    param([string]$Log)

    $samples = @()
    $pattern = (
        "event_code=plugin_lifecycle result_code=(?<result>[a-z]+) " +
        "error_code=(?<error>[a-z_]+) plugin_id=(?<plugin>[a-z0-9.-]+) " +
        "session_id=(?<session>[0-9a-f-]+) stage=(?<stage>[a-z]+) " +
        "active_session=(?:true|false) queue_wait_us=(?<queue>[0-9]+) " +
        "callback_duration_us=(?<callback>[0-9]+) total_duration_us=(?<total>[0-9]+)"
    )
    foreach ($match in [regex]::Matches($Log, $pattern)) {
        $samples += [pscustomobject]@{
            result = $match.Groups["result"].Value
            error_code = $match.Groups["error"].Value
            plugin_id = $match.Groups["plugin"].Value
            session_id = $match.Groups["session"].Value
            stage = $match.Groups["stage"].Value
            queue_wait_us = [double]$match.Groups["queue"].Value
            callback_duration_us = [double]$match.Groups["callback"].Value
            total_duration_us = [double]$match.Groups["total"].Value
        }
    }
    return @($samples)
}

function Get-PluginTimingSummaries {
    param([string]$Log)

    $summaries = @()
    $pattern = (
        "event_code=plugin_callback_timing result_code=ok stage=(?<stage>[a-z]+) " +
        "callback_count=(?<count>[0-9]+) p50_callback_us=(?<callback50>[0-9]+) " +
        "p95_callback_us=(?<callback95>[0-9]+) p99_callback_us=(?<callback99>[0-9]+) " +
        "maximum_callback_us=(?<callbackMax>[0-9]+) p50_queue_us=(?<queue50>[0-9]+) " +
        "p95_queue_us=(?<queue95>[0-9]+) p99_queue_us=(?<queue99>[0-9]+) " +
        "maximum_queue_us=(?<queueMax>[0-9]+)"
    )
    foreach ($match in [regex]::Matches($Log, $pattern)) {
        $summaries += [pscustomobject]@{
            stage = $match.Groups["stage"].Value
            callback_count = [int64]$match.Groups["count"].Value
            callback = [ordered]@{
                sample_count = [int64]$match.Groups["count"].Value
                mean = $null
                p50 = [double]$match.Groups["callback50"].Value
                p95 = [double]$match.Groups["callback95"].Value
                p99 = [double]$match.Groups["callback99"].Value
                max = [double]$match.Groups["callbackMax"].Value
            }
            queue = [ordered]@{
                sample_count = [int64]$match.Groups["count"].Value
                mean = $null
                p50 = [double]$match.Groups["queue50"].Value
                p95 = [double]$match.Groups["queue95"].Value
                p99 = [double]$match.Groups["queue99"].Value
                max = [double]$match.Groups["queueMax"].Value
            }
        }
    }
    return @($summaries)
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
    return ([BitConverter]::ToString((Get-Sha256Bytes $packagePayload))).Replace("-", "").ToLowerInvariant()
}

function Write-Utf8File {
    param(
        [string]$Path,
        [string]$Contents
    )

    [IO.File]::WriteAllText($Path, $Contents, [Text.UTF8Encoding]::new($false))
}

function New-PluginFixture {
    param(
        [ValidateSet("disabled", "idle_enabled", "active", "slow", "faulted")]
        [string]$Scenario,
        [string]$GradialDirectory
    )

    $pluginId = "org.gradial.performance"
    $pluginDirectory = New-Item -ItemType Directory -Path (
        Join-Path $GradialDirectory "plugins\$pluginId"
    ) -Force
    $manifest = @"
manifest_version = 1
id = "$pluginId"
name = "Performance fixture"
version = "0.1.0"
api_version = 1
entry = "main.lua"
permissions = []

[[actions]]
id = "measure"
name = "Measure"
kind = "continuous"
update_hz = 30
"@
    $source = switch ($Scenario) {
        "disabled" { "error('disabled fixture source executed')" }
        "faulted" { "error('faulted fixture source executed')" }
        "slow" {
@"
local function work(iterations, seed)
    local value = seed or 0
    for index = 1, iterations do
        value = (value + index) % 997
    end
    return value
end

return {
    begin = function(context, parameters, initial)
        return { value = initial.value, work = work(40000, 0) }
    end,
    update = function(session, sample)
        session.value = sample.value
        session.work = work(12000, session.work)
    end,
    commit = function(session, sample)
        session.value = sample.value
        session.work = work(40000, session.work)
    end,
    cancel = function(session, reason)
        session.reason = reason
        session.work = work(20000, session.work)
    end,
}
"@
        }
        default {
@"
return {
    begin = function(context, parameters, initial)
        return { value = initial.value, updates = 0 }
    end,
    update = function(session, sample)
        session.value = sample.value
        session.updates = session.updates + 1
    end,
    commit = function(session, sample)
        session.value = sample.value
    end,
    cancel = function(session, reason)
        session.reason = reason
    end,
}
"@
        }
    }
    $manifestPath = Join-Path $pluginDirectory "plugin.toml"
    $sourcePath = Join-Path $pluginDirectory "main.lua"
    Write-Utf8File $manifestPath $manifest
    Write-Utf8File $sourcePath $source
    $manifestBytes = [IO.File]::ReadAllBytes($manifestPath)
    $sourceBytes = [IO.File]::ReadAllBytes($sourcePath)
    $fingerprint = Get-PluginPackageFingerprint $manifestBytes $sourceBytes

    $enabled = $Scenario -in @("idle_enabled", "active", "slow", "faulted")
    $faulted = $Scenario -eq "faulted"
    $lastLoadedManifest = if ($enabled) {
        [ordered]@{
            plugin_id = $pluginId
            manifest_version = 1
            plugin_version = "0.1.0"
            api_version = 1
            package_sha256 = $fingerprint
        }
    } else {
        $null
    }
    $records = [ordered]@{}
    $records[$pluginId] = [ordered]@{
        enabled = $enabled
        approved_permissions = @()
        approved_loopback = @()
        last_loaded_manifest = $lastLoadedManifest
        faulted = $faulted
    }
    $state = [ordered]@{
        schema_version = 1
        revision = 1
        plugins = $records
    }
    Write-Utf8File `
        (Join-Path $GradialDirectory "plugins-state.json") `
        ($state | ConvertTo-Json -Depth 8)

    return [ordered]@{
        plugin_id = $pluginId
        enabled = $enabled
        faulted = $faulted
        fingerprint = $fingerprint
    }
}

function New-ScenarioConfiguration {
    param(
        [ValidateSet("disabled", "idle_enabled", "active", "slow", "faulted")]
        [string]$Scenario,
        [string]$GradialDirectory,
        [string]$PluginId
    )

    $continuous = $Scenario -in @("active", "slow")
    $continuousActionId = "67726164-6961-4c00-8000-000000000401"
    $delayActionId = "67726164-6961-4c00-8000-000000000402"
    $profileId = "67726164-6961-4c00-8000-000000000004"
    $slices = @("N", "NE", "E", "SE", "S", "SW", "W", "NW") | ForEach-Object {
        $slice = [ordered]@{ label = $_; action_id = $delayActionId }
        if ($continuous) {
            $slice.continuous_action_id = $continuousActionId
        }
        $slice
    }
    $actions = @(
        [ordered]@{
            id = $delayActionId
            name = "Performance delay"
            kind = [ordered]@{ type = "delay"; milliseconds = 10 }
        }
    )
    if ($continuous) {
        $actions += [ordered]@{
                id = $continuousActionId
                name = "Plugin performance fixture"
                kind = [ordered]@{
                    type = "plugin_continuous"
                    plugin_id = $PluginId
                    plugin_action_id = "measure"
                    parameters = [ordered]@{}
                }
            }
    }
    $configuration = [ordered]@{
        schema_version = 6
        config_revision = 1
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
            name = "Plugin performance fixture"
            menu = [ordered]@{ slice_count = 8; slices = @($slices) }
        }
        application_profiles = @()
        actions = $actions
    }
    Write-Utf8File `
        (Join-Path $GradialDirectory "config.json") `
        ($configuration | ConvertTo-Json -Depth 12)
}

function Get-ScenarioResult {
    param(
        [string]$Scenario,
        [string]$Configuration,
        [Collections.IDictionary]$Preset,
        [string]$Workspace,
        [string]$ReportDirectory,
        [bool]$UsePresentMon,
        [string]$PresentMonExecutable,
        [int]$TargetMonitor
    )

    $scenarioDirectory = New-Item -ItemType Directory -Path (
        Join-Path $ReportDirectory "$Configuration-$Scenario"
    ) -Force
    $captureDirectory = New-Item -ItemType Directory -Path (
        Join-Path $scenarioDirectory "capture"
    ) -Force
    $temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) (
        "gradial-plugin-performance-" + [guid]::NewGuid().ToString("N")
    )
    $gradialDirectory = Join-Path $temporaryRoot "Gradial"
    $processIdFile = Join-Path $scenarioDirectory "gradial.pid"
    $phaseFile = Join-Path $scenarioDirectory "phase.txt"
    $stdoutPath = Join-Path $scenarioDirectory "interaction.stdout.txt"
    $stderrPath = Join-Path $scenarioDirectory "interaction.stderr.txt"
    $resourcePath = Join-Path $scenarioDirectory "resources.csv"
    $originalLocalAppData = $env:LOCALAPPDATA
    $child = $null
    $resources = @()
    $succeeded = $false
    try {
        New-Item -ItemType Directory -Path $gradialDirectory -Force | Out-Null
        Write-Utf8File `
            (Join-Path $gradialDirectory "onboarding-v1.json") `
            '{"schema_version":1,"completed":true}'
        $fixture = New-PluginFixture $Scenario $gradialDirectory
        New-ScenarioConfiguration $Scenario $gradialDirectory $fixture.plugin_id

        $gestureMilliseconds = $Preset.gesture_delay_ms
        $captureDuration = [Math]::Min(
            600,
            [Math]::Max(
                5,
                [Math]::Ceiling(
                    $Preset.invocation_count *
                    ((2 * $gestureMilliseconds) + $Preset.inter_invocation_delay_ms) /
                    1000.0
                ) + 3
            )
        )
        $arguments = @(
            "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
            ('"' + (Join-Path $PSScriptRoot "smoke-interaction.ps1") + '"'),
            "-Configuration", $Configuration,
            "-Geometry", "fixed-origin",
            "-Slices", "8",
            "-Direction", "E",
            "-UseConfiguredMenu",
            "-InvocationCount", $Preset.invocation_count,
            "-GestureDelayMilliseconds", $gestureMilliseconds,
            "-InterInvocationDelayMilliseconds", $Preset.inter_invocation_delay_ms,
            "-WarmupSeconds", $Preset.warmup_seconds,
            "-PostInvocationIdleSeconds", $Preset.idle_seconds,
            "-CaptureTiming",
            "-CaptureDurationSeconds", $captureDuration,
            "-MonitorIndex", $TargetMonitor,
            "-ProcessIdFile", ('"' + $processIdFile + '"'),
            "-PhaseFile", ('"' + $phaseFile + '"'),
            "-TimingOutputDirectory", ('"' + $captureDirectory + '"')
        )
        if ($Scenario -in @("active", "slow")) {
            $arguments += @("-PluginContinuous", "-ContinuousPercent", "75")
        } else {
            $arguments += "-ExpectDelayExecution"
        }
        if (-not $UsePresentMon) {
            $arguments += "-TimingMarkersOnly"
        } else {
            $arguments += @("-PresentMonPath", ('"' + $PresentMonExecutable + '"'))
        }

        $env:LOCALAPPDATA = $temporaryRoot
        $child = Start-Process -FilePath "powershell.exe" `
            -ArgumentList $arguments `
            -WorkingDirectory $Workspace `
            -RedirectStandardOutput $stdoutPath `
            -RedirectStandardError $stderrPath `
            -PassThru `
            -WindowStyle Hidden
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (
            [DateTime]::UtcNow -lt $deadline -and
            -not (Test-Path -LiteralPath $processIdFile) -and
            -not $child.HasExited
        ) {
            Start-Sleep -Milliseconds 50
        }
        if (-not (Test-Path -LiteralPath $processIdFile)) {
            throw "$Configuration/$Scenario did not publish Gradial's process ID."
        }
        $gradialProcessId = [int](Get-Content -Raw -LiteralPath $processIdFile)
        $previousTime = [DateTime]::UtcNow
        $previousCpu = 0.0
        while (-not $child.HasExited) {
            $process = Get-Process -Id $gradialProcessId -ErrorAction SilentlyContinue
            if ($process) {
                $now = [DateTime]::UtcNow
                $cpuMs = $process.TotalProcessorTime.TotalMilliseconds
                $wallMs = ($now - $previousTime).TotalMilliseconds
                $cpuPercent = if ($wallMs -gt 0 -and $previousCpu -gt 0) {
                    (($cpuMs - $previousCpu) / $wallMs) * 100.0 /
                        [Environment]::ProcessorCount
                } else {
                    $null
                }
                $phase = if (Test-Path -LiteralPath $phaseFile) {
                    (Get-Content -Raw -LiteralPath $phaseFile).Trim()
                } else {
                    "startup"
                }
                $resources += [pscustomobject]@{
                    captured_at = $now.ToString("o")
                    phase = $phase
                    working_set_mib = $process.WorkingSet64 / 1MB
                    private_memory_mib = $process.PrivateMemorySize64 / 1MB
                    cpu_percent = $cpuPercent
                }
                $previousTime = $now
                $previousCpu = $cpuMs
            }
            Start-Sleep -Milliseconds $Preset.sample_interval_ms
            $child.Refresh()
        }
        $child.WaitForExit()
        $child.Refresh()
        $output = if (Test-Path -LiteralPath $stdoutPath) {
            Get-Content -Raw -LiteralPath $stdoutPath
        } else {
            ""
        }
        $childSucceeded = if ($null -ne $child.ExitCode) {
            $child.ExitCode -eq 0
        } else {
            $output -match "smoke check passed"
        }
        if (-not $childSucceeded -or $output -notmatch "smoke check passed") {
            $errorOutput = if (Test-Path -LiteralPath $stderrPath) {
                Get-Content -Raw -LiteralPath $stderrPath
            } else {
                ""
            }
            throw "$Configuration/$Scenario interaction failed with code $($child.ExitCode). $errorOutput"
        }
        $resources | Export-Csv -LiteralPath $resourcePath -NoTypeInformation -Encoding utf8
        $logEvidenceDirectory = New-Item -ItemType Directory -Path (
            Join-Path $scenarioDirectory "logs"
        ) -Force
        $logParts = @(
            Get-ChildItem -LiteralPath (Join-Path $gradialDirectory "logs") -Filter "gradial*.log" |
                Sort-Object LastWriteTime
        )
        foreach ($logPart in $logParts) {
            Copy-Item -LiteralPath $logPart.FullName -Destination $logEvidenceDirectory
        }
        $log = ($logParts | ForEach-Object {
            Get-Content -Raw -LiteralPath $_.FullName
        }) -join [Environment]::NewLine
        $succeeded = $true
    }
    finally {
        $env:LOCALAPPDATA = $originalLocalAppData
        if ($child -and -not $child.HasExited) {
            Stop-Process -Id $child.Id -Force
        }
    }

    try {
        $markerCsv = Get-ChildItem -LiteralPath $captureDirectory -Filter "gradial-markers-*.csv" |
            Sort-Object LastWriteTime | Select-Object -Last 1
        $markers = Read-TimingMarkers $markerCsv.FullName
        $presentCsv = Get-ChildItem -LiteralPath $captureDirectory -Filter "gradial-presents-*.csv" |
            Sort-Object LastWriteTime | Select-Object -Last 1
        $qpcFrequency = $markers | Select-Object -First 1 -ExpandProperty qpc_frequency
        $presents = Get-PresentEvents $presentCsv.FullName $qpcFrequency
        $lifecycle = Get-PluginLifecycleSamples $log
        $timingSummaries = @(Get-PluginTimingSummaries $log)
        $allTiming = $timingSummaries | Where-Object stage -eq "all" | Select-Object -Last 1
        $idle = @($resources | Where-Object phase -eq "idle")
        $idleCpu = [double[]]@(
            $idle | Select-Object -Skip 1 | Where-Object { $null -ne $_.cpu_percent } |
                ForEach-Object { [double]$_.cpu_percent }
        )
        $idleWorking = [double[]]@($idle | ForEach-Object { [double]$_.working_set_mib })
        $idlePrivate = [double[]]@($idle | ForEach-Object { [double]$_.private_memory_mib })
        $open = Get-MarkerDurations $markers "activation_dequeued" source_qpc "overlay_shown" marker_qpc
        $visualStart = if ($Scenario -in @("active", "slow")) {
            "continuous_value_requested"
        } else {
            "highlight_requested"
        }
        $visualEnd = if ($Scenario -in @("active", "slow")) {
            "continuous_value_applied"
        } else {
            "highlight_applied"
        }
        $visual = Get-MarkerDurations $markers $visualStart source_qpc $visualEnd marker_qpc
        $displayedOpen = Get-DisplayedDurations `
            $markers $presents "overlay_shown" "activation_dequeued"
        $displayedVisual = Get-DisplayedDurations `
            $markers $presents $visualEnd $visualStart
        $hook = [regex]::Matches(
            $log,
            "event_code=hook_thread_stopped .*callback_count=(?<count>[0-9]+) " +
            "p99_callback_us=(?<p99>[0-9]+) maximum_callback_us=(?<max>[0-9]+)"
        ) | Select-Object -Last 1
        $workerReady = [regex]::Matches(
            $log,
            "event_code=plugin_worker_ready .*loaded_plugins=(?<count>[0-9]+)"
        ) | Select-Object -Last 1
        $status = [regex]::Matches(
            $log,
            "event_code=plugin_state_status .*enabled=(?<enabled>[0-9]+) " +
            "disabled=(?<disabled>[0-9]+) reload_required=(?<reload>[0-9]+) " +
            "faulted=(?<faulted>[0-9]+)"
        ) | Select-Object -Last 1
        $startupLoads = ([regex]::Matches(
            $log,
            "event_code=plugin_worker_operation .*operation=enable .*source=startup"
        )).Count
        $expectedCallbacks = $Scenario -in @("active", "slow")
        $expectedLoaded = $Scenario -in @("idle_enabled", "active", "slow")
        $beginCount = (
            $timingSummaries | Where-Object stage -eq "begin" |
                Select-Object -Last 1 -ExpandProperty callback_count
        )
        $updateCount = (
            $timingSummaries | Where-Object stage -eq "update" |
                Select-Object -Last 1 -ExpandProperty callback_count
        )
        $commitCount = (
            $timingSummaries | Where-Object stage -eq "commit" |
                Select-Object -Last 1 -ExpandProperty callback_count
        )
        $cancelCount = (
            $timingSummaries | Where-Object stage -eq "cancel" |
                Select-Object -Last 1 -ExpandProperty callback_count
        )
        $executionContract = if ($expectedCallbacks) {
            $allTiming.callback_count -gt 0 -and
            $beginCount -ge $Preset.invocation_count -and
            $commitCount -ge $Preset.invocation_count
        } else {
            $allTiming.callback_count -eq 0
        }
        $loadedCount = if ($workerReady) { [int]$workerReady.Groups["count"].Value } else { -1 }
        $loadContract = if ($expectedLoaded) {
            $loadedCount -eq 1 -and $startupLoads -eq 1
        } else {
            $loadedCount -eq 0 -and $startupLoads -eq 0
        }
        $health = [ordered]@{
            input_queue_overflows = ([regex]::Matches($log, "event_code=input_queue_overflow")).Count
            hook_heartbeat_misses = ([regex]::Matches($log, "event_code=hook_thread_unresponsive")).Count
            focus_failures = ([regex]::Matches($log, "event_code=focus_checkpoint .*accepted=false")).Count
            renderer_failures = ([regex]::Matches(
                $log,
                "target=renderer .*result_code=failure"
            )).Count
        }
        $healthContract = ($health.Values | Measure-Object -Sum).Sum -eq 0
        $frameContract = (
            $open.Count -ge $Preset.invocation_count -and
            $visual.Count -ge $Preset.invocation_count
        )
        $privacyContract = $log -notmatch (
            "disabled fixture source executed|faulted fixture source executed|" +
            "local function work"
        )
        $result = [ordered]@{
            scenario = $Scenario
            configuration = $Configuration
            expected = [ordered]@{
                plugin_loaded = $expectedLoaded
                callbacks = $expectedCallbacks
            }
            plugin = [ordered]@{
                loaded_plugins = $loadedCount
                startup_loads = $startupLoads
                state_enabled = if ($status) { [int]$status.Groups["enabled"].Value } else { -1 }
                state_disabled = if ($status) { [int]$status.Groups["disabled"].Value } else { -1 }
                state_faulted = if ($status) { [int]$status.Groups["faulted"].Value } else { -1 }
                lifecycle_callbacks = $allTiming.callback_count
                retained_lifecycle_results_ok = @($lifecycle | Where-Object result -eq "ok").Count
                retained_lifecycle_results_failed = @(
                    $lifecycle | Where-Object result -eq "failure"
                ).Count
                stage_counts = [ordered]@{
                    begin = $beginCount
                    update = $updateCount
                    commit = $commitCount
                    cancel = $cancelCount
                }
            }
            contracts = [ordered]@{
                execution = if ($executionContract) { "pass" } else { "fail" }
                load_state = if ($loadContract) { "pass" } else { "fail" }
                frame_checkpoints = if ($frameContract) { "pass" } else { "fail" }
                input_renderer_health = if ($healthContract) { "pass" } else { "fail" }
                log_privacy = if ($privacyContract) { "pass" } else { "fail" }
            }
            metrics = [ordered]@{
                idle_cpu_percent = Get-Statistics $idleCpu
                idle_working_set_mib = Get-Statistics $idleWorking
                idle_private_memory_mib = Get-Statistics $idlePrivate
                callback_duration_us = $allTiming.callback
                callback_queue_wait_us = $allTiming.queue
                application_open_checkpoint_ms = Get-Statistics $open
                application_visual_checkpoint_ms = Get-Statistics $visual
                displayed_open_ms = Get-Statistics $displayedOpen
                displayed_visual_ms = Get-Statistics $displayedVisual
                hook_callback_us = if ($hook) {
                    [ordered]@{
                        sample_count = [int64]$hook.Groups["count"].Value
                        mean = $null
                        p50 = $null
                        p95 = $null
                        p99 = [double]$hook.Groups["p99"].Value
                        max = [double]$hook.Groups["max"].Value
                    }
                } else {
                    $null
                }
            }
            frame_evidence = [ordered]@{
                timing_markers = $markers.Count
                open_samples = $open.Count
                visual_samples = $visual.Count
                present_events = $presents.Count
                presentmon = if ($presents.Count -gt 0) {
                    "correlated"
                } elseif ($UsePresentMon) {
                    "captured_without_correlatable_events"
                } else {
                    "unavailable"
                }
            }
            health = $health
            evidence_directory = $scenarioDirectory.FullName
        }
        Write-Utf8File `
            (Join-Path $scenarioDirectory "scenario-result.json") `
            ($result | ConvertTo-Json -Depth 12)
        return $result
    }
    finally {
        $resolved = Resolve-Path -LiteralPath $temporaryRoot -ErrorAction SilentlyContinue
        $systemTemporary = Resolve-Path -LiteralPath ([IO.Path]::GetTempPath())
        if (
            $resolved -and
            $resolved.Path.StartsWith($systemTemporary.Path, [StringComparison]::OrdinalIgnoreCase) -and
            (Split-Path -Leaf $resolved.Path).StartsWith("gradial-plugin-performance-")
        ) {
            Remove-Item -LiteralPath $resolved.Path -Recurse -Force
        }
        if (-not $succeeded) {
            Write-Warning "$Configuration/$Scenario did not complete."
        }
    }
}

function Format-Number {
    param([object]$Value)

    if ($null -eq $Value) { return "-" }
    return [Math]::Round([double]$Value, 3)
}

if ($SelfTest) {
    $statistics = Get-Statistics ([double[]](1..100))
    if (
        $statistics.p50 -ne 50 -or $statistics.p95 -ne 95 -or
        $statistics.p99 -ne 99 -or $statistics.max -ne 100
    ) {
        throw "Plugin performance percentile self-test failed."
    }
    $sampleLog = (
        "event_code=plugin_lifecycle result_code=ok error_code=none " +
        "plugin_id=org.gradial.performance session_id=00000000-0000-0000-0000-000000000001 " +
        "stage=update active_session=true queue_wait_us=12 callback_duration_us=34 total_duration_us=46"
    )
    $sample = @(Get-PluginLifecycleSamples $sampleLog)
    if (
        $sample.Count -ne 1 -or $sample[0].stage -ne "update" -or
        $sample[0].queue_wait_us -ne 12 -or $sample[0].callback_duration_us -ne 34
    ) {
        throw "Plugin lifecycle analyzer self-test failed."
    }
    $summaryLog = (
        "event_code=plugin_callback_timing result_code=ok stage=all callback_count=7 " +
        "p50_callback_us=25 p95_callback_us=100 p99_callback_us=250 " +
        "maximum_callback_us=211 p50_queue_us=50 p95_queue_us=100 " +
        "p99_queue_us=250 maximum_queue_us=201"
    )
    $summary = @(Get-PluginTimingSummaries $summaryLog)
    if (
        $summary.Count -ne 1 -or $summary[0].callback_count -ne 7 -or
        $summary[0].callback.p95 -ne 100 -or $summary[0].queue.max -ne 201
    ) {
        throw "Plugin timing aggregate analyzer self-test failed."
    }
    $manifest = [Text.UTF8Encoding]::new($false).GetBytes("manifest")
    $source = [Text.UTF8Encoding]::new($false).GetBytes("source")
    $first = Get-PluginPackageFingerprint $manifest $source
    $second = Get-PluginPackageFingerprint $manifest $source
    $changed = Get-PluginPackageFingerprint $manifest ([byte[]]@(1, 2, 3))
    if ($first.Length -ne 64 -or $first -ne $second -or $first -eq $changed) {
        throw "Plugin fixture fingerprint self-test failed."
    }
    Write-Host "Plugin performance analyzer self-test passed."
    return
}

if ($Mode -eq "Release" -and $Build -ne "release") {
    throw "The release-duration protocol requires -Build release. Use Development mode to compare debug and release builds."
}

$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$configurations = if ($Build -eq "Both") { @("debug", "release") } else { @($Build) }
foreach ($configuration in $configurations) {
    $executable = Join-Path $workspace "target\$configuration\gradial.exe"
    if (-not (Test-Path -LiteralPath $executable)) {
        throw "Build Gradial's $configuration configuration before running the plugin report."
    }
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the isolated plugin report."
}

$principal = New-Object Security.Principal.WindowsPrincipal(
    [Security.Principal.WindowsIdentity]::GetCurrent()
)
$isElevated = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $PresentMonPath) {
    $PresentMonPath = @(
        (Join-Path $env:LOCALAPPDATA "Programs\PresentMon\PresentMon.exe"),
        "C:\Program Files\PresentMon\PresentMon.exe",
        "C:\Program Files\NVIDIA Corporation\FrameViewSDK\bin\PresentMon_x64.exe"
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
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
        gesture_delay_ms = 50
        inter_invocation_delay_ms = 25
        sample_interval_ms = 1000
    }
} else {
    [ordered]@{
        invocation_count = 10
        warmup_seconds = 1
        idle_seconds = 3
        gesture_delay_ms = 50
        inter_invocation_delay_ms = 25
        sample_interval_ms = 250
    }
}
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$reportDirectory = New-Item -ItemType Directory -Path (
    Join-Path $OutputDirectory "$timestamp-$($Mode.ToLowerInvariant())"
) -Force
$results = @()
$scenarios = @("disabled", "idle_enabled", "active", "slow", "faulted")
foreach ($configuration in $configurations) {
    foreach ($scenario in $scenarios) {
        Write-Host "Measuring $configuration/$scenario..."
        $results += Get-ScenarioResult `
            -Scenario $scenario `
            -Configuration $configuration `
            -Preset $preset `
            -Workspace $workspace `
            -ReportDirectory $reportDirectory `
            -UsePresentMon $usePresentMon `
            -PresentMonExecutable $PresentMonPath `
            -TargetMonitor $MonitorIndex
    }
}

$comparisons = @()
foreach ($configuration in $configurations) {
    $disabled = $results | Where-Object {
        $_.configuration -eq $configuration -and $_.scenario -eq "disabled"
    }
    $idleEnabled = $results | Where-Object {
        $_.configuration -eq $configuration -and $_.scenario -eq "idle_enabled"
    }
    $active = $results | Where-Object {
        $_.configuration -eq $configuration -and $_.scenario -eq "active"
    }
    $slow = $results | Where-Object {
        $_.configuration -eq $configuration -and $_.scenario -eq "slow"
    }
    $workingSetDelta = if (
        $disabled.metrics.idle_working_set_mib -and
        $idleEnabled.metrics.idle_working_set_mib
    ) {
        $idleEnabled.metrics.idle_working_set_mib.max -
            $disabled.metrics.idle_working_set_mib.max
    } else {
        $null
    }
    $resourceGate = if ($Mode -eq "Release" -and $null -ne $workingSetDelta) {
        if ($workingSetDelta -le 8) { "pass" } else { "fail" }
    } else {
        "not_evaluated"
    }
    $slowDiagnostic = if (
        $active.metrics.callback_duration_us -and
        $slow.metrics.callback_duration_us
    ) {
        if ($slow.metrics.callback_duration_us.p50 -gt $active.metrics.callback_duration_us.p50) {
            "confirmed"
        } else {
            "not_observed"
        }
    } else {
        "unavailable"
    }
    $comparisons += [ordered]@{
        configuration = $configuration
        idle_enabled_working_set_delta_mib = $workingSetDelta
        idle_enabled_working_set_gate = $resourceGate
        slow_fixture_timing = $slowDiagnostic
    }
}

$contractFailures = @(
    $results | Where-Object {
        $_.contracts.execution -eq "fail" -or
        $_.contracts.load_state -eq "fail" -or
        $_.contracts.frame_checkpoints -eq "fail" -or
        $_.contracts.input_renderer_health -eq "fail" -or
        $_.contracts.log_privacy -eq "fail"
    }
)
$overall = if ($contractFailures.Count -gt 0) {
    "fail"
} elseif ($Mode -eq "Development") {
    "not_evaluated"
} elseif ($comparisons.idle_enabled_working_set_gate -contains "fail") {
    "fail"
} else {
    "not_evaluated"
}
$report = [ordered]@{
    report_schema_version = 1
    captured_at = (Get-Date).ToUniversalTime().ToString("o")
    overall_gate_result = $overall
    protocol = [ordered]@{
        mode = $Mode
        configurations = $configurations
        scenarios = $scenarios
        invocation_count_per_scenario = $preset.invocation_count
        warmup_seconds_per_scenario = $preset.warmup_seconds
        idle_seconds_per_scenario = $preset.idle_seconds
        release_duration_protocol = $Mode -eq "Release"
        complete_release_qualification = $false
    }
    capabilities = [ordered]@{
        elevated = $isElevated
        presentmon = if ($usePresentMon) { "captured" } elseif ($presentMonAvailable) {
            "installed_but_elevation_required"
        } else {
            "not_installed"
        }
        timing_markers = "captured_per_scenario"
    }
    results = $results
    comparisons = $comparisons
    notes = @(
        "Development mode checks the harness and correctness contracts but does not qualify release resource or latency gates.",
        "Release mode evaluates the release-duration resource contract, but overall qualification remains not evaluated until displayed-frame and full input-to-callback latency gates are available.",
        "Disabled and faulted fixtures contain top-level failing Lua; zero callbacks and zero startup loads prove those sources stayed inert.",
        "Application checkpoints are diagnostic and do not substitute for PresentMon displayed-frame evidence.",
        "Callback queue wait starts when the continuous coordinator submits to the bounded plugin lane; it does not yet include the configured update-period wait."
    )
}
$jsonPath = Join-Path $reportDirectory "plugin-performance-report.json"
Write-Utf8File $jsonPath ($report | ConvertTo-Json -Depth 14)

$csvRows = foreach ($result in $results) {
    foreach ($metricProperty in $result.metrics.GetEnumerator()) {
        $statistics = $metricProperty.Value
        [pscustomobject]@{
            configuration = $result.configuration
            scenario = $result.scenario
            metric = $metricProperty.Key
            sample_count = $statistics.sample_count
            mean = $statistics.mean
            p50 = $statistics.p50
            p95 = $statistics.p95
            p99 = $statistics.p99
            max = $statistics.max
        }
    }
}
$csvPath = Join-Path $reportDirectory "plugin-performance-metrics.csv"
$csvRows | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding utf8

$lines = @(
    "# Gradial plugin performance report",
    "",
    "- Captured: $($report.captured_at)",
    "- Mode: $Mode",
    "- Configurations: $($configurations -join ', ')",
    "- Overall gate result: **$overall**",
    "- PresentMon: $($report.capabilities.presentmon)",
    "",
    "| Build | Scenario | Callbacks | Callback p95 us | Queue p95 us | Idle CPU mean % | Idle WS max MiB | Open p95 ms | Visual p95 ms | Displayed visual p95 ms | Hook p99 us | Contracts |",
    "|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|"
)
foreach ($result in $results) {
    $contracts = @($result.contracts.Values) -join "/"
    $callbackP95 = if ($result.plugin.lifecycle_callbacks -gt 0) {
        Format-Number $result.metrics.callback_duration_us.p95
    } else {
        "-"
    }
    $queueP95 = if ($result.plugin.lifecycle_callbacks -gt 0) {
        Format-Number $result.metrics.callback_queue_wait_us.p95
    } else {
        "-"
    }
    $lines += (
        "| $($result.configuration) | $($result.scenario) | " +
        "$($result.plugin.lifecycle_callbacks) | " +
        "$callbackP95 | $queueP95 | " +
        "$(Format-Number $result.metrics.idle_cpu_percent.mean) | " +
        "$(Format-Number $result.metrics.idle_working_set_mib.max) | " +
        "$(Format-Number $result.metrics.application_open_checkpoint_ms.p95) | " +
        "$(Format-Number $result.metrics.application_visual_checkpoint_ms.p95) | " +
        "$(Format-Number $result.metrics.displayed_visual_ms.p95) | " +
        "$(Format-Number $result.metrics.hook_callback_us.p99) | $contracts |"
    )
}
$lines += @("", "## Comparisons", "")
foreach ($comparison in $comparisons) {
    $lines += (
        "- $($comparison.configuration): idle-enabled working-set delta " +
        "$(Format-Number $comparison.idle_enabled_working_set_delta_mib) MiB " +
        "($($comparison.idle_enabled_working_set_gate)); slow timing " +
        "$($comparison.slow_fixture_timing)."
    )
}
$lines += @("", "## Notes", "")
foreach ($note in $report.notes) {
    $lines += "- $note"
}
$markdownPath = Join-Path $reportDirectory "plugin-performance-report.md"
[IO.File]::WriteAllLines($markdownPath, $lines, [Text.UTF8Encoding]::new($false))

Write-Host "Gradial $Mode plugin performance report complete."
Write-Host "  Markdown: $markdownPath"
Write-Host "  JSON:     $jsonPath"
Write-Host "  CSV:      $csvPath"
Write-Host "  Overall:  $overall"
if ($contractFailures.Count -gt 0) {
    throw "$($contractFailures.Count) plugin performance scenario contract(s) failed."
}
