param(
    [ValidateRange(1, 600)]
    [int]$DurationSeconds = 15,

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\captures"),

    [string]$PresentMonPath,

    [ValidateRange(0, 4294967295)]
    [long]$ProcessId = 0,

    [string]$ProcessName,

    [string]$ReadyFile,

    [switch]$AllProcesses,

    [switch]$PresentMonOnly,

    [switch]$MarkersOnly
)

$ErrorActionPreference = "Stop"
$providerGuid = "{3c55b348-69f8-4d33-8e81-62ba761d4f52}"
$sessionName = "GradialTiming-$PID"
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"

if ($MarkersOnly -and $PresentMonOnly) {
    throw "MarkersOnly and PresentMonOnly cannot be combined."
}

if (-not $MarkersOnly -and -not $PresentMonPath) {
    $presentMonCommand = Get-Command PresentMon -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($presentMonCommand) {
        $PresentMonPath = $presentMonCommand.Source
    }
    else {
        $nvidiaPresentMon = "C:\Program Files\NVIDIA Corporation\FrameViewSDK\bin\PresentMon_x64.exe"
        if (Test-Path -LiteralPath $nvidiaPresentMon) {
            $PresentMonPath = $nvidiaPresentMon
        }
    }
}

if (-not $MarkersOnly -and
    (-not $PresentMonPath -or -not (Test-Path -LiteralPath $PresentMonPath))) {
    throw "PresentMon was not found. Pass its executable path with -PresentMonPath."
}
$output = New-Item -ItemType Directory -Path $OutputDirectory -Force
$markerEtl = Join-Path $output "gradial-markers-$timestamp.etl"
$markerCsv = Join-Path $output "gradial-markers-$timestamp.csv"
$presentCsv = Join-Path $output "gradial-presents-$timestamp.csv"
$presentArguments = Join-Path $output "presentmon-$timestamp.arguments.txt"
$presentStdout = Join-Path $output "presentmon-$timestamp.stdout.txt"
$presentStderr = Join-Path $output "presentmon-$timestamp.stderr.txt"

if (-not $PresentMonOnly) {
    & logman start $sessionName -ets -o $markerEtl -p $providerGuid 0x1 0x4
    if ($LASTEXITCODE -ne 0) {
        throw "Could not start the Gradial ETW marker session. Try an elevated PowerShell."
    }
}

try {
    Write-Host "Capturing Gradial timing for $DurationSeconds seconds. Complete several M4 gestures now."
    if ($MarkersOnly) {
        if ($ReadyFile) {
            [IO.File]::WriteAllText(
                $ReadyFile,
                "markers_ready`r`n",
                [Text.UTF8Encoding]::new($false)
            )
        }
        Start-Sleep -Seconds $DurationSeconds
    }
    else {
        $arguments = @(
            "--timed", $DurationSeconds,
            "--terminate_after_timed",
            "--qpc_time",
            "--v1_metrics",
            "--output_file", $presentCsv,
            "--no_console_stats",
            "--session_name", "GradialPresent-$PID"
        )
        if ($AllProcesses) {
            # The report analyzer filters the completed capture to the exact
            # Gradial PID. PresentMon attachment can miss initially hidden
            # swap chains, so automated runs observe the shared ETW session.
        }
        elseif ($ProcessName) {
            $arguments = @("--process_name", $ProcessName) + $arguments
        }
        elseif ($ProcessId -gt 0) {
            $arguments = @("--process_id", $ProcessId) + $arguments
        }
        else {
            $arguments = @("--process_name", "gradial.exe") + $arguments
        }
        [IO.File]::WriteAllLines(
            $presentArguments,
            [string[]]$arguments,
            [Text.UTF8Encoding]::new($false)
        )
        $startArguments = @($arguments | ForEach-Object {
            $argument = [string]$_
            if ($argument -match '[\s&]') {
                '"' + $argument.Replace('"', '\"') + '"'
            }
            else {
                $argument
            }
        })
        $presentMon = Start-Process `
            -FilePath $PresentMonPath `
            -Verb RunAs `
            -ArgumentList $startArguments `
            -PassThru `
            -WindowStyle Hidden
        if ($ReadyFile) {
            [IO.File]::WriteAllText(
                $ReadyFile,
                "presentmon_started process_id=$($presentMon.Id)`r`n",
                [Text.UTF8Encoding]::new($false)
            )
        }
        $presentMon.WaitForExit()
        $presentMonExitCode = $presentMon.ExitCode
        [IO.File]::WriteAllText(
            $presentStdout,
            "PresentMon ShellExecute launch completed with exit code ${presentMonExitCode}.`r`n",
            [Text.UTF8Encoding]::new($false)
        )
        [IO.File]::WriteAllText(
            $presentStderr,
            "Console redirection is unavailable for the elevated ShellExecute launch.`r`n",
            [Text.UTF8Encoding]::new($false)
        )
        if ($presentMonExitCode -ne 0) {
            throw "PresentMon exited with code ${presentMonExitCode}."
        }
        if (
            -not (Test-Path -LiteralPath $presentCsv) -or
            (Get-Item -LiteralPath $presentCsv).Length -eq 0
        ) {
            $presentOutput = if (Test-Path -LiteralPath $presentStdout) {
                (Get-Content -LiteralPath $presentStdout -Raw).Trim()
            } else { "no standard output" }
            $presentError = if (Test-Path -LiteralPath $presentStderr) {
                (Get-Content -LiteralPath $presentStderr -Raw).Trim()
            } else { "no error output" }
            throw (
                "PresentMon completed without a CSV. " +
                "stdout='$presentOutput' stderr='$presentError'"
            )
        }
    }
}
finally {
    if (-not $PresentMonOnly) {
        & logman stop $sessionName -ets
    }
}

if (-not $PresentMonOnly) {
    & tracerpt $markerEtl -o $markerCsv -of CSV -y
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "The ETL capture succeeded, but tracerpt could not create the marker CSV."
    }
}

Write-Host "Timing capture complete:"
if (-not $PresentMonOnly) {
    Write-Host "  Gradial markers: $markerEtl"
    Write-Host "  Marker table:    $markerCsv"
}
if (-not $MarkersOnly) {
    Write-Host "  Presented frames: $presentCsv"
}
