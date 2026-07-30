param(
    [ValidateRange(1, 600)]
    [int]$DurationSeconds = 15,

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\captures"),

    [string]$PresentMonPath,

    [ValidateRange(0, 4294967295)]
    [long]$ProcessId = 0,

    [switch]$MarkersOnly
)

$ErrorActionPreference = "Stop"
$providerGuid = "{3c55b348-69f8-4d33-8e81-62ba761d4f52}"
$sessionName = "GradialTiming-$PID"
$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"

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
if (-not $MarkersOnly) {
    $principal = New-Object Security.Principal.WindowsPrincipal(
        [Security.Principal.WindowsIdentity]::GetCurrent()
    )
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Live PresentMon capture requires an elevated PowerShell. Use -MarkersOnly when only the Gradial ETW stream is needed."
    }
}

$output = New-Item -ItemType Directory -Path $OutputDirectory -Force
$markerEtl = Join-Path $output "gradial-markers-$timestamp.etl"
$markerCsv = Join-Path $output "gradial-markers-$timestamp.csv"
$presentCsv = Join-Path $output "gradial-presents-$timestamp.csv"

& logman start $sessionName -ets -o $markerEtl -p $providerGuid 0x1 0x4
if ($LASTEXITCODE -ne 0) {
    throw "Could not start the Gradial ETW marker session. Try an elevated PowerShell."
}

try {
    Write-Host "Capturing Gradial timing for $DurationSeconds seconds. Complete several M4 gestures now."
    if ($MarkersOnly) {
        Start-Sleep -Seconds $DurationSeconds
    }
    else {
        $arguments = @(
            "--timed", $DurationSeconds,
            "--terminate_after_timed",
            "--qpc_time",
            "--output_file", "`"$presentCsv`"",
            "--no_console_stats"
        )
        if ($ProcessId -gt 0) {
            $arguments = @("--process_id", $ProcessId) + $arguments
        }
        else {
            $arguments = @("--process_name", "gradial.exe") + $arguments
        }
        $presentMon = Start-Process -FilePath $PresentMonPath `
            -ArgumentList $arguments `
            -PassThru `
            -WindowStyle Hidden
        $presentMon.WaitForExit()
        if ($presentMon.ExitCode -ne 0) {
            throw "PresentMon exited with code $($presentMon.ExitCode)."
        }
    }
}
finally {
    & logman stop $sessionName -ets
}

& tracerpt $markerEtl -o $markerCsv -of CSV -y
if ($LASTEXITCODE -ne 0) {
    Write-Warning "The ETL capture succeeded, but tracerpt could not create the marker CSV."
}

Write-Host "Timing capture complete:"
Write-Host "  Gradial markers: $markerEtl"
Write-Host "  Marker table:    $markerCsv"
if (-not $MarkersOnly) {
    Write-Host "  Presented frames: $presentCsv"
}
