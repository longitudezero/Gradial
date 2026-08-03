param(
    [string]$InputDirectory = (Join-Path $env:LOCALAPPDATA "Gradial\continuous-studies"),

    [string]$OutputPath = "",

    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

function Get-Median {
    param([double[]]$Values)

    if ($Values.Count -eq 0) {
        return $null
    }
    $ordered = @($Values | Sort-Object)
    $midpoint = [Math]::Floor($ordered.Count / 2)
    if ($ordered.Count % 2 -eq 0) {
        return ($ordered[$midpoint - 1] + $ordered[$midpoint]) / 2.0
    }
    return $ordered[$midpoint]
}

function Get-Fraction {
    param(
        [int]$Numerator,
        [int]$Denominator
    )

    if ($Denominator -eq 0) {
        return $null
    }
    return $Numerator / [double]$Denominator
}

function New-Gate {
    param(
        [string]$Requirement,
        [bool]$EnoughParticipants,
        [bool]$Passed,
        [string]$Evidence
    )

    [pscustomobject]@{
        requirement = $Requirement
        status = if (-not $EnoughParticipants) {
            "not_evaluated"
        } elseif ($Passed) {
            "passed"
        } else {
            "failed"
        }
        evidence = $Evidence
    }
}

function Test-ParticipantReport {
    param(
        [object]$Report,
        [string]$Source
    )

    if ($Report.schema_version -ne 2 -or $Report.protocol -ne "gradial-continuous-usability-v2") {
        throw "Unsupported continuous-study report: $Source"
    }
    if ($Report.planned_trials -ne 37 -or $Report.summary.accepted_trials -ne 37) {
        throw "Incomplete continuous-study report: $Source"
    }
    $acceptedAttempts = @($Report.attempts | Where-Object accepted -eq $true)
    $expectedPhases = [ordered]@{
        practice = 4
        scalar = 24
        direction_change = 8
        cancellation = 1
    }
    foreach ($phase in $expectedPhases.Keys) {
        $observed = @($acceptedAttempts | Where-Object phase -eq $phase).Count
        if ($observed -ne $expectedPhases[$phase]) {
            throw "Incomplete continuous-study report: $Source"
        }
    }
    if ($acceptedAttempts.Count -ne 37) {
        throw "Incomplete continuous-study report: $Source"
    }
    $preArmChanges = @(
        $acceptedAttempts |
            Where-Object { $_.phase -eq "direction_change" -and $_.requested_change_after_arm -ne $true }
    )
    $armedChanges = @(
        $acceptedAttempts |
            Where-Object { $_.phase -eq "direction_change" -and $_.requested_change_after_arm -eq $true }
    )
    if ($preArmChanges.Count -ne 4 -or $armedChanges.Count -ne 4) {
        throw "Incomplete continuous-study report: $Source"
    }
    $excludedData = @($Report.privacy.excluded_data)
    $requiredExclusions = @(
        "participant_name",
        "raw_cursor_coordinates",
        "application_identity",
        "file_paths",
        "window_details",
        "monitor_device_names"
    )
    if (
        $Report.privacy.identity -ne "random_session_id_only" -or
        @($requiredExclusions | Where-Object { $_ -notin $excludedData }).Count -ne 0
    ) {
        throw "The privacy declaration is missing or incompatible in $Source."
    }
}

function Get-CohortSummary {
    param([object[]]$Reports)

    $participantCount = $Reports.Count
    $enoughParticipants = $participantCount -ge 8
    $accepted = @(
        $Reports | ForEach-Object { $_.attempts } | Where-Object { $_.accepted -eq $true }
    )
    $scalar = @($accepted | Where-Object phase -eq "scalar")
    $eightDirection = @($scalar | Where-Object requested_slice_count -eq 8)
    $eightDirectionErrors = @(
        $eightDirection | Where-Object direction_correct -eq $false
    ).Count
    $eightDirectionErrorRate = Get-Fraction $eightDirectionErrors $eightDirection.Count

    $scalarErrors = @($scalar | ForEach-Object { [double]$_.absolute_scalar_error })
    $medianScalarError = Get-Median $scalarErrors
    $withinCount = @($scalarErrors | Where-Object { $_ -le 0.15 }).Count
    $withinFraction = Get-Fraction $withinCount $scalarErrors.Count

    $directionChange = @($accepted | Where-Object phase -eq "direction_change")
    $preArmDirectionChange = @(
        $directionChange | Where-Object requested_change_after_arm -ne $true
    )
    $accidentalArms = @($preArmDirectionChange | Where-Object result -eq "committed").Count
    $accidentalArmingRate = Get-Fraction $accidentalArms $preArmDirectionChange.Count
    $armedDirectionChange = @(
        $directionChange | Where-Object requested_change_after_arm -eq $true
    )
    $armedDirectionSwitchSuccesses = @(
        $armedDirectionChange |
            Where-Object { $_.result -eq "committed" -and $_.direction_correct -eq $true }
    ).Count
    $armedDirectionSwitchSuccessRate = Get-Fraction `
        $armedDirectionSwitchSuccesses `
        $armedDirectionChange.Count

    $successfulCancellationParticipants = @(
        $Reports | Where-Object {
            @(
                $_.attempts | Where-Object {
                    $_.accepted -eq $true -and
                    $_.phase -eq "cancellation" -and
                    $_.cancel_reason -eq "returned_to_center"
                }
            ).Count -gt 0
        }
    ).Count
    $cancellationParticipantFraction = Get-Fraction `
        $successfulCancellationParticipants `
        $participantCount

    $centerMedian = Get-Median @(
        $scalar | Where-Object requested_placement -eq "center" |
            ForEach-Object { [double]$_.duration_ms }
    )
    $edgeMedian = Get-Median @(
        $scalar | Where-Object requested_placement -eq "edge" |
            ForEach-Object { [double]$_.duration_ms }
    )
    $cornerMedian = Get-Median @(
        $scalar | Where-Object requested_placement -eq "corner" |
            ForEach-Object { [double]$_.duration_ms }
    )
    $outerMedian = if ($null -ne $edgeMedian -and $null -ne $cornerMedian) {
        [Math]::Max($edgeMedian, $cornerMedian)
    } else {
        $null
    }
    $slowdownFraction = if ($null -ne $centerMedian -and $centerMedian -gt 0 -and $null -ne $outerMedian) {
        $outerMedian / $centerMedian - 1.0
    } else {
        $null
    }

    $metrics = [ordered]@{
        participants = $participantCount
        measured_scalar_samples = $scalar.Count
        eight_direction_samples = $eightDirection.Count
        eight_direction_errors = $eightDirectionErrors
        eight_direction_error_rate = $eightDirectionErrorRate
        median_absolute_scalar_error = $medianScalarError
        samples_within_0_15 = $withinCount
        fraction_within_0_15 = $withinFraction
        accidental_arming_trials = $preArmDirectionChange.Count
        accidental_arms = $accidentalArms
        accidental_arming_rate = $accidentalArmingRate
        armed_direction_switch_trials = $armedDirectionChange.Count
        armed_direction_switch_successes = $armedDirectionSwitchSuccesses
        armed_direction_switch_success_rate = $armedDirectionSwitchSuccessRate
        successful_cancellation_participants = $successfulCancellationParticipants
        cancellation_participant_fraction = $cancellationParticipantFraction
        center_median_completion_ms = $centerMedian
        edge_median_completion_ms = $edgeMedian
        corner_median_completion_ms = $cornerMedian
        edge_corner_slowdown_fraction = $slowdownFraction
    }

    $gates = @(
        New-Gate `
            -Requirement "at least eight participants" `
            -EnoughParticipants $true `
            -Passed $enoughParticipants `
            -Evidence "$participantCount anonymous participant reports were supplied."
        New-Gate `
            -Requirement "eight-direction error at most 3%" `
            -EnoughParticipants $enoughParticipants `
            -Passed ($null -ne $eightDirectionErrorRate -and $eightDirectionErrorRate -le 0.03) `
            -Evidence "$eightDirectionErrors errors across $($eightDirection.Count) measured eight-slice samples."
        New-Gate `
            -Requirement "median absolute scalar error at most 0.06" `
            -EnoughParticipants $enoughParticipants `
            -Passed ($null -ne $medianScalarError -and $medianScalarError -le 0.06) `
            -Evidence "Observed median absolute error: $medianScalarError."
        New-Gate `
            -Requirement "at least 95% of final samples within 0.15" `
            -EnoughParticipants $enoughParticipants `
            -Passed ($null -ne $withinFraction -and $withinFraction -ge 0.95) `
            -Evidence "$withinCount of $($scalarErrors.Count) measured samples were within 0.15."
        New-Gate `
            -Requirement "accidental arming below 2%" `
            -EnoughParticipants $enoughParticipants `
            -Passed ($null -ne $accidentalArmingRate -and $accidentalArmingRate -lt 0.02) `
            -Evidence "$accidentalArms accidental arms across $($preArmDirectionChange.Count) pre-arm direction-change trials."
        New-Gate `
            -Requirement "at least 95% of armed direction switches finish in the requested wedge" `
            -EnoughParticipants $enoughParticipants `
            -Passed (
                $null -ne $armedDirectionSwitchSuccessRate -and
                $armedDirectionSwitchSuccessRate -ge 0.95
            ) `
            -Evidence "$armedDirectionSwitchSuccesses of $($armedDirectionChange.Count) armed direction switches finished in the requested wedge."
        New-Gate `
            -Requirement "at least seven of eight participants cancel after one instruction" `
            -EnoughParticipants $enoughParticipants `
            -Passed (
                $null -ne $cancellationParticipantFraction -and
                $cancellationParticipantFraction -ge 0.875
            ) `
            -Evidence "$successfulCancellationParticipants of $participantCount participants cancelled by returning to center."
        New-Gate `
            -Requirement "edge and corner median no more than 20% slower than center" `
            -EnoughParticipants $enoughParticipants `
            -Passed ($null -ne $slowdownFraction -and $slowdownFraction -le 0.20) `
            -Evidence "Center median $centerMedian ms; edge $edgeMedian ms; corner $cornerMedian ms."
    )

    [ordered]@{
        schema_version = 2
        source_schema_version = 2
        protocol = "gradial-continuous-usability-v2"
        metrics = $metrics
        gates = $gates
    }
}

function New-SelfTestReport {
    param([int]$Participant)

    $attempts = [System.Collections.Generic.List[object]]::new()
    for ($index = 0; $index -lt 4; $index++) {
        $attempts.Add([pscustomobject]@{
            accepted = $true
            phase = "practice"
            requested_slice_count = 8
            direction_correct = $true
            absolute_scalar_error = 0.0
            requested_placement = "center"
            duration_ms = 100
            result = "committed"
            cancel_reason = $null
        })
    }
    for ($index = 0; $index -lt 24; $index++) {
        $placement = @("center", "edge", "corner")[$index % 3]
        $attempts.Add([pscustomobject]@{
            accepted = $true
            phase = "scalar"
            requested_slice_count = if ($index % 2 -eq 0) { 4 } else { 8 }
            direction_correct = $true
            absolute_scalar_error = 0.04
            requested_placement = $placement
            duration_ms = 100 + $(if ($placement -eq "center") { 0 } else { 10 })
            result = "committed"
            cancel_reason = $null
        })
    }
    for ($index = 0; $index -lt 8; $index++) {
        $changeAfterArm = $index % 2 -eq 1
        $attempts.Add([pscustomobject]@{
            accepted = $true
            phase = "direction_change"
            requested_slice_count = 8
            requested_change_after_arm = $changeAfterArm
            direction_correct = if ($changeAfterArm) { $true } else { $null }
            absolute_scalar_error = $null
            requested_placement = "center"
            duration_ms = 80
            result = if ($changeAfterArm) { "committed" } else { "cancelled" }
            cancel_reason = if ($changeAfterArm) { $null } else { "no_selection" }
        })
    }
    $attempts.Add([pscustomobject]@{
        accepted = $true
        phase = "cancellation"
        requested_slice_count = 8
        direction_correct = $null
        absolute_scalar_error = $null
        requested_placement = "center"
        duration_ms = 90
        result = "cancelled"
        cancel_reason = "returned_to_center"
    })
    [pscustomobject]@{
        schema_version = 2
        protocol = "gradial-continuous-usability-v2"
        session_id = "00000000-0000-0000-0000-$($Participant.ToString('000000000000'))"
        planned_trials = 37
        attempts = $attempts
        summary = [pscustomobject]@{ accepted_trials = 37 }
        privacy = [pscustomobject]@{
            identity = "random_session_id_only"
            excluded_data = @(
                "participant_name",
                "raw_cursor_coordinates",
                "application_identity",
                "file_paths",
                "window_details",
                "monitor_device_names"
            )
        }
    }
}

if ($SelfTest) {
    $reports = @(0..7 | ForEach-Object { New-SelfTestReport $_ })
    foreach ($report in $reports) {
        Test-ParticipantReport -Report $report -Source "self-test"
    }
    $summary = Get-CohortSummary $reports
    $failed = @($summary.gates | Where-Object status -ne "passed")
    if ($failed.Count -ne 0) {
        throw "Continuous study summarizer self-test failed: $($failed.requirement -join ', ')."
    }
    if ($summary.metrics.measured_scalar_samples -ne 192) {
        throw "Continuous study summarizer self-test counted the wrong scalar samples."
    }
    $undersized = Get-CohortSummary @($reports | Select-Object -First 7)
    if (
        $undersized.gates[0].status -ne "failed" -or
        @($undersized.gates | Select-Object -Skip 1 | Where-Object status -ne "not_evaluated").Count -ne 0
    ) {
        throw "Continuous study summarizer self-test did not defer an undersized cohort."
    }
    Write-Host "Continuous study summarizer self-test passed."
    exit 0
}

if (-not (Test-Path -LiteralPath $InputDirectory -PathType Container)) {
    throw "The continuous-study input directory does not exist: $InputDirectory"
}
$reportFiles = @(
    Get-ChildItem -LiteralPath $InputDirectory -Filter "continuous-study-*.json" -File |
        Where-Object Name -Match '^continuous-study-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.json$' |
        Sort-Object Name
)
if ($reportFiles.Count -eq 0) {
    throw "No continuous-study participant reports were found in $InputDirectory."
}

$reports = [System.Collections.Generic.List[object]]::new()
$sessionIds = [System.Collections.Generic.HashSet[string]]::new(
    [StringComparer]::OrdinalIgnoreCase
)
foreach ($file in $reportFiles) {
    $report = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    Test-ParticipantReport -Report $report -Source $file.Name
    if (-not $sessionIds.Add([string]$report.session_id)) {
        throw "Duplicate anonymous session ID in $($file.Name)."
    }
    $reports.Add($report)
}

$cohort = Get-CohortSummary $reports
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $runId = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
    $OutputPath = Join-Path $InputDirectory "continuous-study-cohort-$runId.json"
}
$resolvedOutput = [IO.Path]::GetFullPath($OutputPath)
if ([IO.Path]::GetExtension($resolvedOutput) -ne ".json") {
    throw "The cohort output path must end in .json."
}
if (Test-Path -LiteralPath $resolvedOutput) {
    throw "The cohort output already exists: $resolvedOutput"
}
$outputDirectory = Split-Path -Parent $resolvedOutput
[IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
$temporaryJson = "$resolvedOutput.tmp"
$csvPath = [IO.Path]::ChangeExtension($resolvedOutput, ".gates.csv")
$temporaryCsv = "$csvPath.tmp"
if (
    (Test-Path -LiteralPath $temporaryJson) -or
    (Test-Path -LiteralPath $csvPath) -or
    (Test-Path -LiteralPath $temporaryCsv)
) {
    throw "A cohort output or temporary file already exists."
}

try {
    $json = $cohort | ConvertTo-Json -Depth 8
    [IO.File]::WriteAllText(
        $temporaryJson,
        $json + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false)
    )
    $cohort.gates | Export-Csv -LiteralPath $temporaryCsv -NoTypeInformation -Encoding utf8
    Move-Item -LiteralPath $temporaryJson -Destination $resolvedOutput
    Move-Item -LiteralPath $temporaryCsv -Destination $csvPath
}
catch {
    Remove-Item -LiteralPath $temporaryJson, $temporaryCsv -Force -ErrorAction SilentlyContinue
    throw
}

Write-Host "Continuous study cohort summarized: $($reports.Count) participant(s)."
Write-Host "JSON summary: $resolvedOutput"
Write-Host "Gate table:   $csvPath"
if ($reports.Count -lt 8) {
    Write-Host "The Section 11.3 gates remain not evaluated until at least eight reports are supplied."
}
