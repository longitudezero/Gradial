param()

$ErrorActionPreference = "Stop"

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class GradialOpenSmokeAssociations
{
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(
        uint eventId,
        uint flags,
        IntPtr item1,
        IntPtr item2
    );
}
"@

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$executable = Join-Path $workspace "target\release\gradial.exe"
if (-not (Test-Path -LiteralPath $executable)) {
    throw "Build Gradial's release configuration before running the open-action smoke check."
}

$originalLocalAppData = $env:LOCALAPPDATA
$temporaryPrefix = "gradial-open-smoke-"
$identity = [guid]::NewGuid().ToString("N")
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    $temporaryPrefix + $identity
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$configPath = Join-Path $gradialDirectory "config.json"
$ackPath = Join-Path $temporaryRoot "open-ack.txt"
$privacySentinel = "GRADIAL_PRIVATE_OPEN_SENTINEL"
$extension = ".gradialsmoke$identity"
$payloadPath = Join-Path $temporaryRoot (
    "$privacySentinel %PATH% & literal$extension"
)
$programId = "Gradial.Smoke.$identity"
$extensionKey = "Registry::HKEY_CURRENT_USER\Software\Classes\$extension"
$programKey = "Registry::HKEY_CURRENT_USER\Software\Classes\$programId"
$commandKey = Join-Path $programKey "shell\open\command"
$createdExtensionKey = $false
$createdProgramKey = $false
$actionId = "67726164-6961-4c00-8000-000000000104"
$profileId = "67726164-6961-4c00-8000-000000000001"
$labels = @("N", "NE", "E", "SE", "S", "SW", "W", "NW")

try {
    New-Item -ItemType Directory -Path $gradialDirectory -Force | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $gradialDirectory "onboarding-v1.json"),
        '{"schema_version":1,"completed":true}',
        [System.Text.UTF8Encoding]::new($false)
    )
    [System.IO.File]::WriteAllText(
        $payloadPath,
        "Gradial isolated shell-association fixture",
        [System.Text.UTF8Encoding]::new($false)
    )

    $receiverScript = Join-Path $PSScriptRoot "open-smoke-receiver.ps1"
    $windowsPowerShell = Join-Path $env:SystemRoot (
        "System32\WindowsPowerShell\v1.0\powershell.exe"
    )
    $handlerCommand = (
        '"' + $windowsPowerShell + '" -NoProfile -NonInteractive ' +
        '-WindowStyle Hidden -ExecutionPolicy Bypass -File "' +
        $receiverScript + '" -OpenedPath "%1" -AckPath "' + $ackPath + '"'
    )

    New-Item -Path $extensionKey -Force | Out-Null
    $createdExtensionKey = $true
    Set-Item -LiteralPath $extensionKey -Value $programId
    New-Item -Path $commandKey -Force | Out-Null
    $createdProgramKey = $true
    Set-Item -LiteralPath $commandKey -Value $handlerCommand
    [GradialOpenSmokeAssociations]::SHChangeNotify(
        0x08000000,
        0,
        [IntPtr]::Zero,
        [IntPtr]::Zero
    )

    $slices = @(
        $labels | ForEach-Object {
            [ordered]@{
                label = $_
                action_id = $actionId
            }
        }
    )
    $configuration = [ordered]@{
        schema_version = 2
        config_revision = 1
        global_profile = [ordered]@{
            id = $profileId
            name = "Open path smoke"
            menu = [ordered]@{
                slice_count = 8
                slices = $slices
            }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "Open path smoke action"
                kind = [ordered]@{
                    type = "open_path"
                    path = $payloadPath
                }
            }
        )
    }
    [System.IO.File]::WriteAllText(
        $configPath,
        ($configuration | ConvertTo-Json -Depth 10),
        [System.Text.UTF8Encoding]::new($false)
    )

    $env:LOCALAPPDATA = $temporaryRoot
    & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
        -Configuration release `
        -Geometry fixed-origin `
        -Slices 8 `
        -Direction E `
        -InvocationCount 1 `
        -ExpectOpenExecution

    $ackDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while (
        -not (Test-Path -LiteralPath $ackPath) -and
        [DateTime]::UtcNow -lt $ackDeadline
    ) {
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path -LiteralPath $ackPath)) {
        throw "The isolated Windows file association did not receive Gradial's open request."
    }
    $openedPath = [System.IO.File]::ReadAllText($ackPath)
    if ($openedPath -cne $payloadPath) {
        throw "The isolated Windows file association did not receive the exact literal path."
    }

    $logPath = Join-Path $temporaryRoot "Gradial\logs\gradial_rCURRENT.log"
    $log = Get-Content -Raw -LiteralPath $logPath
    if ($log.Contains($privacySentinel)) {
        throw "The open-action path appeared in Gradial's log."
    }
    if (
        $log -notmatch (
            "event_code=shell_open_step_started .*request_kind=path"
        )
    ) {
        throw "Gradial did not record the redacted open-path step metadata."
    }
    "Windows received the exact literal path through an isolated shell association."
}
finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    if (
        $createdExtensionKey -and
        (Split-Path -Leaf $extensionKey).StartsWith(".gradialsmoke")
    ) {
        Remove-Item -LiteralPath $extensionKey -Recurse -Force -ErrorAction SilentlyContinue
    }
    if (
        $createdProgramKey -and
        (Split-Path -Leaf $programKey).StartsWith("Gradial.Smoke.")
    ) {
        Remove-Item -LiteralPath $programKey -Recurse -Force -ErrorAction SilentlyContinue
    }
    [GradialOpenSmokeAssociations]::SHChangeNotify(
        0x08000000,
        0,
        [IntPtr]::Zero,
        [IntPtr]::Zero
    )

    $resolvedTemporary = Resolve-Path -LiteralPath $temporaryRoot -ErrorAction SilentlyContinue
    $resolvedSystemTemporary =
        Resolve-Path -LiteralPath ([System.IO.Path]::GetTempPath()) -ErrorAction Stop
    if (
        $resolvedTemporary -and
        $resolvedTemporary.Path.StartsWith(
            $resolvedSystemTemporary.Path,
            [System.StringComparison]::OrdinalIgnoreCase
        ) -and
        (Split-Path -Leaf $resolvedTemporary.Path).StartsWith($temporaryPrefix)
    ) {
        Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
    }
}
