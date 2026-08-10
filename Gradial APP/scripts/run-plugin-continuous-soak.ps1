param(
    [ValidateRange(1, 10000)]
    [int]$InvocationCount = 10000,

    [string]$OutputDirectory = (Join-Path $PSScriptRoot "..\captures\plugin-continuous-soak"),

    [ValidateRange(0, 15)]
    [int]$MonitorIndex = 0,

    [switch]$SelfTest
)

$ErrorActionPreference = "Stop"

$runner = Join-Path $PSScriptRoot "run-plugin-performance-report.ps1"
if ($SelfTest) {
    & $runner -SelfTest
    return
}

& $runner `
    -Mode Soak `
    -Build release `
    -SoakInvocationCount $InvocationCount `
    -OutputDirectory $OutputDirectory `
    -MonitorIndex $MonitorIndex `
    -MarkersOnly
