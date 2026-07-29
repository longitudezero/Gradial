param(
    [Parameter(Mandatory = $true)]
    [string]$OpenedPath,

    [Parameter(Mandatory = $true)]
    [string]$AckPath
)

$ErrorActionPreference = "Stop"

[System.IO.File]::WriteAllText(
    $AckPath,
    $OpenedPath,
    [System.Text.UTF8Encoding]::new($false)
)
