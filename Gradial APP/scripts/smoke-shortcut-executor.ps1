param()

$ErrorActionPreference = "Stop"

& (Join-Path $PSScriptRoot "smoke-input-executor.ps1") -Mode shortcut
