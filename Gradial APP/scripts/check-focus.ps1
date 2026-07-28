param(
    [string]$LogPath
)

$ErrorActionPreference = "Stop"

if (-not $LogPath) {
    $LogPath = Join-Path $env:LOCALAPPDATA "Gradial\logs\gradial_rCURRENT.log"
}

$resolvedLog = Resolve-Path -LiteralPath $LogPath -ErrorAction Stop
$lines = Get-Content -LiteralPath $resolvedLog
$closeLine = $lines |
    Where-Object { $_ -match "event_code=focus_checkpoint .*phase=after_close .*accepted=true" } |
    Select-Object -Last 1

if (-not $closeLine -or $closeLine -notmatch "invocation_id=([0-9a-f-]{36})") {
    throw "No completed focus-check invocation was found in $resolvedLog. Run Gradial, complete one M4 gesture, then retry."
}

$invocationId = $Matches[1]
$invocationLines = $lines | Where-Object { $_ -match "invocation_id=$invocationId(?:\s|$)" }

foreach ($phase in @("activation", "after_first_frame", "after_close")) {
    $checkpoint = $invocationLines |
        Where-Object {
            $_ -match "event_code=focus_checkpoint " -and
            $_ -match "phase=$phase " -and
            $_ -match "accepted=true"
        } |
        Select-Object -Last 1
    if (-not $checkpoint) {
        throw "Invocation $invocationId is missing an accepted '$phase' focus checkpoint."
    }
}

if ($invocationLines | Where-Object { $_ -match "event_code=focus_changed " }) {
    throw "Invocation $invocationId contains a foreground-window change."
}

Write-Host "Focus check passed for invocation $invocationId."
