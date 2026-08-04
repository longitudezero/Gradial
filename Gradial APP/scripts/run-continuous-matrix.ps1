param(
    [ValidateSet("quick", "standard", "extended")]
    [string]$Suite = "standard",

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\captures\continuous-matrix")
)

$ErrorActionPreference = "Stop"

Add-Type -AssemblyName System.Windows.Forms

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$smokeScript = Join-Path $PSScriptRoot "smoke-interaction.ps1"
$executable = Join-Path $workspace "target\debug\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's debug configuration before running the continuous matrix."
}
if (Get-Process gradial -ErrorAction SilentlyContinue) {
    throw "Close the existing Gradial process before running the isolated continuous matrix."
}

$runId = [DateTime]::UtcNow.ToString("yyyyMMddTHHmmssfffZ")
$runDirectory = Join-Path $OutputDirectory $runId
New-Item -ItemType Directory -Path $runDirectory -Force | Out-Null
$startedUtc = [DateTimeOffset]::UtcNow
$scenarioResults = [System.Collections.Generic.List[object]]::new()

function Invoke-ContinuousScenario {
    param(
        [string]$Id,
        [ValidateSet("4", "8")]
        [string]$Slices,
        [ValidateRange(0, 100)]
        [int]$Value,
        [string]$ActivationPoint,
        [int]$ExpectedInvocations,
        [string]$Direction = "",
        [string]$ChangeFromDirection = "",
        [int]$PointerBurstEvents = 0,
        [string]$Notes = ""
    )

    Write-Host "[$($scenarioResults.Count + 1)] $Id"
    $arguments = @(
        "-Configuration", "debug",
        "-Geometry", "auto",
        "-Slices", $Slices,
        "-ContinuousMeter",
        "-ContinuousPercent", $Value,
        "-ActivationPoint", $ActivationPoint,
        "-GestureDelayMilliseconds", "25",
        "-InterInvocationDelayMilliseconds", "100"
    )
    if ([string]::IsNullOrWhiteSpace($ChangeFromDirection)) {
        $arguments += "-AllDirections"
    } else {
        $arguments += @("-Direction", $Direction, "-ChangeFromDirection", $ChangeFromDirection)
    }
    if ($PointerBurstEvents -ne 0) {
        $arguments += @("-PointerBurstEvents", $PointerBurstEvents)
    }
    $scenarioStarted = [DateTimeOffset]::UtcNow
    $priorPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass `
            -File $smokeScript @arguments 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $priorPreference
    }
    $scenarioEnded = [DateTimeOffset]::UtcNow
    $scenarioResults.Add([pscustomobject]@{
        id = $Id
        status = if ($exitCode -eq 0) { "passed" } else { "failed" }
        slices = [int]$Slices
        target_value = $Value / 100.0
        activation_point = $ActivationPoint
        expected_invocations = $ExpectedInvocations
        started_utc = $scenarioStarted.ToString("O")
        duration_ms = [Math]::Round(($scenarioEnded - $scenarioStarted).TotalMilliseconds, 3)
        arguments = $arguments -join " "
        notes = $Notes
        exit_code = $exitCode
        output = (@($output) -join [Environment]::NewLine).Trim()
    })
}

$values = if ($Suite -eq "quick") { @(0, 50, 100) } else { @(0, 25, 50, 75, 100) }
foreach ($value in $values) {
    Invoke-ContinuousScenario `
        -Id "center-eight-value-$value" `
        -Slices "8" `
        -Value $value `
        -ActivationPoint "center" `
        -ExpectedInvocations 8
}

if ($Suite -ne "quick") {
    Invoke-ContinuousScenario `
        -Id "center-four-value-50" `
        -Slices "4" `
        -Value 50 `
        -ActivationPoint "center" `
        -ExpectedInvocations 4

    Invoke-ContinuousScenario `
        -Id "center-eight-armed-turn-N-to-W-value-50" `
        -Slices "8" `
        -Value 50 `
        -ActivationPoint "center" `
        -ExpectedInvocations 1 `
        -Direction "W" `
        -ChangeFromDirection "N" `
        -Notes "Arms north, changes to west at the same radius, and commits west."

    Invoke-ContinuousScenario `
        -Id "center-eight-fixed-origin-pointer-burst-value-50" `
        -Slices "8" `
        -Value 50 `
        -ActivationPoint "center" `
        -ExpectedInvocations 1 `
        -Direction "E" `
        -ChangeFromDirection "E" `
        -PointerBurstEvents 1000 `
        -Notes "Commits east after 1,000 uncoalesced Windows pointer inputs without visual backlog."

    foreach ($point in @("top-left", "top-right", "bottom-right", "bottom-left")) {
        Invoke-ContinuousScenario `
            -Id "$point-eight-value-75" `
            -Slices "8" `
            -Value 75 `
            -ActivationPoint $point `
            -ExpectedInvocations 8 `
            -Notes "Exercises the 200 DIP continuous edge-assist extent."
    }
}

if ($Suite -eq "extended") {
    foreach ($point in @("top-edge", "right-edge", "bottom-edge", "left-edge")) {
        foreach ($value in @(25, 50, 75, 100)) {
            Invoke-ContinuousScenario `
                -Id "$point-eight-value-$value" `
                -Slices "8" `
                -Value $value `
                -ActivationPoint $point `
                -ExpectedInvocations 8
        }
    }
}

$screens = [System.Windows.Forms.Screen]::AllScreens
$monitorRecords = for ($index = 0; $index -lt $screens.Count; $index++) {
    $screen = $screens[$index]
    [pscustomobject]@{
        index = $index
        device_name = $screen.DeviceName
        primary = $screen.Primary
        bounds = [pscustomobject]@{
            left = $screen.Bounds.Left
            top = $screen.Bounds.Top
            width = $screen.Bounds.Width
            height = $screen.Bounds.Height
        }
        work_area = [pscustomobject]@{
            left = $screen.WorkingArea.Left
            top = $screen.WorkingArea.Top
            width = $screen.WorkingArea.Width
            height = $screen.WorkingArea.Height
        }
    }
}

$failed = @($scenarioResults | Where-Object status -eq "failed").Count
$passed = @($scenarioResults | Where-Object status -eq "passed").Count
$expectedInvocations = (
    $scenarioResults | Measure-Object -Property expected_invocations -Sum
).Sum
$coverage = @(
    [pscustomobject]@{
        requirement = "0%, 25%, 50%, 75%, and 100% normalized targets"
        status = if ($Suite -eq "quick") {
            "partial"
        } elseif ($failed -eq 0) {
            "passed"
        } else {
            "failed"
        }
        evidence = "Center scenarios exercised values: $($values -join ', ')%."
    },
    [pscustomobject]@{
        requirement = "four- and eight-slice continuous directions"
        status = if ($Suite -eq "quick") {
            "partial"
        } elseif ($failed -eq 0) {
            "passed"
        } else {
            "failed"
        }
        evidence = if ($Suite -eq "quick") {
            "Eight slices exercised; four slices are excluded from the quick suite."
        } else {
            "All directions in four- and eight-slice menus exercised."
        }
    },
    [pscustomobject]@{
        requirement = "continuous edge assist from every corner"
        status = if ($Suite -eq "quick") {
            "not_run"
        } elseif ($failed -eq 0) {
            "passed"
        } else {
            "failed"
        }
        evidence = if ($Suite -eq "quick") {
            "Corners are excluded from the quick suite."
        } else {
            "All eight directions exercised at 75% from all four corners."
        }
    },
    [pscustomobject]@{
        requirement = "live wedge change after continuous arming"
        status = if ($Suite -eq "quick") {
            "not_run"
        } elseif ($failed -eq 0) {
            "passed"
        } else {
            "failed"
        }
        evidence = if ($Suite -eq "quick") {
            "Armed direction switching is excluded from the quick suite."
        } else {
            "A 50% north arm changed to west and committed the west wedge."
        }
    },
    [pscustomobject]@{
        requirement = "fixed-origin high-polling input coalescing"
        status = if ($Suite -eq "quick") {
            "not_run"
        } elseif ($failed -eq 0) {
            "passed"
        } else {
            "failed"
        }
        evidence = if ($Suite -eq "quick") {
            "The fixed-origin pointer burst is excluded from the quick suite."
        } else {
            "A 1,000-event no-coalescing Windows pointer burst committed the live value and reported obsolete visual-update coalescing."
        }
    },
    [pscustomobject]@{
        requirement = "human scalar precision and accidental arming study"
        status = "not_run"
        evidence = "Requires at least eight participants; automation does not infer usability."
    }
)

$endedUtc = [DateTimeOffset]::UtcNow
$report = [ordered]@{
    schema_version = 1
    run_id = $runId
    suite = $Suite
    started_utc = $startedUtc.ToString("O")
    ended_utc = $endedUtc.ToString("O")
    duration_ms = [Math]::Round(($endedUtc - $startedUtc).TotalMilliseconds, 3)
    host = [ordered]@{
        computer_name = $env:COMPUTERNAME
        monitors = @($monitorRecords)
    }
    summary = [ordered]@{
        scenarios = $scenarioResults.Count
        passed = $passed
        failed = $failed
        expected_invocations = $expectedInvocations
    }
    scenarios = $scenarioResults
    coverage = $coverage
}

$jsonPath = Join-Path $runDirectory "continuous-matrix.json"
$scenarioCsvPath = Join-Path $runDirectory "scenarios.csv"
$coverageCsvPath = Join-Path $runDirectory "coverage.csv"
$report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $jsonPath -Encoding utf8
$scenarioResults | Export-Csv -LiteralPath $scenarioCsvPath -NoTypeInformation -Encoding utf8
$coverage | Export-Csv -LiteralPath $coverageCsvPath -NoTypeInformation -Encoding utf8

Write-Host ""
Write-Host "Continuous matrix complete: $passed passed, $failed failed."
Write-Host "Expected inert commits: $expectedInvocations"
Write-Host "JSON report:    $jsonPath"
Write-Host "Scenario table: $scenarioCsvPath"
Write-Host "Coverage table: $coverageCsvPath"

if ($failed -gt 0) {
    exit 1
}
