param()

$ErrorActionPreference = "Stop"

$workspace = Resolve-Path (Join-Path $PSScriptRoot "..")
$gradialExecutable = Join-Path $workspace "target\release\gradial.exe"
if (-not (Test-Path -LiteralPath $gradialExecutable)) {
    throw "Build Gradial's release configuration before running the process-action smoke check."
}

$originalLocalAppData = $env:LOCALAPPDATA
$originalInheritedValue = $env:GRADIAL_PROCESS_SMOKE_INHERITED
$temporaryPrefix = "gradial-process-smoke-"
$identity = [guid]::NewGuid().ToString("N")
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    $temporaryPrefix + $identity
)
$gradialDirectory = Join-Path $temporaryRoot "Gradial"
$configPath = Join-Path $gradialDirectory "config.json"
$privacySentinel = "GRADIAL_PRIVATE_PROCESS_SENTINEL_$identity"
$receiverPath = Join-Path $temporaryRoot "$privacySentinel receiver.exe"
$workingDirectory = Join-Path $temporaryRoot "$privacySentinel working %PATH%"
$ackPath = Join-Path $temporaryRoot "$privacySentinel acknowledgement.txt"
$receiverProcessId = $null
$actionId = "67726164-6961-4c00-8000-000000000105"
$profileId = "67726164-6961-4c00-8000-000000000001"
$labels = @("N", "NE", "E", "SE", "S", "SW", "W", "NW")
$unicodeArgument = (
    [string][char]0x03A9 + " caf" + [char]0x00E9 + " " +
    [System.Char]::ConvertFromUtf32(0x1F980)
)
$arguments = @(
    "",
    "plain",
    "two words",
    'quote"value',
    "trailing\",
    'slashes\\before"quote',
    "%PATH%",
    '$env:PATH',
    "shell characters & | > remain literal",
    $unicodeArgument,
    $privacySentinel
)

function ConvertFrom-Base64Utf8 {
    param([string]$Value)

    return [System.Text.Encoding]::UTF8.GetString(
        [System.Convert]::FromBase64String($Value)
    )
}

try {
    New-Item -ItemType Directory -Path $gradialDirectory -Force | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $gradialDirectory "onboarding-v1.json"),
        '{"schema_version":1,"completed":true}',
        [System.Text.UTF8Encoding]::new($false)
    )
    New-Item -ItemType Directory -Path $workingDirectory -Force | Out-Null

    $receiverSource = @"
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Text;
using System.Threading;

public static class GradialProcessSmokeReceiver
{
    public static void Main(string[] arguments)
    {
        if (arguments.Length == 0) return;
        var acknowledgement = arguments[0];
        var temporary = acknowledgement + ".tmp";
        var lines = new List<string> {
            Process.GetCurrentProcess().Id.ToString(CultureInfo.InvariantCulture),
            Encode(Environment.CurrentDirectory),
            Encode(Environment.GetEnvironmentVariable("GRADIAL_PROCESS_SMOKE_INHERITED") ?? ""),
            (arguments.Length - 1).ToString(CultureInfo.InvariantCulture)
        };
        for (var index = 1; index < arguments.Length; index++) {
            lines.Add(Encode(arguments[index]));
        }
        File.WriteAllLines(temporary, lines, new UTF8Encoding(false));
        File.Move(temporary, acknowledgement);
        Thread.Sleep(TimeSpan.FromSeconds(30));
    }

    private static string Encode(string value)
    {
        return Convert.ToBase64String(Encoding.UTF8.GetBytes(value));
    }
}
"@
    Add-Type `
        -TypeDefinition $receiverSource `
        -Language CSharp `
        -OutputAssembly $receiverPath `
        -OutputType WindowsApplication

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
            name = "Process launch smoke"
            menu = [ordered]@{
                slice_count = 8
                slices = $slices
            }
        }
        application_profiles = @()
        actions = @(
            [ordered]@{
                id = $actionId
                name = "Direct process smoke action"
                kind = [ordered]@{
                    type = "run_executable"
                    executable = $receiverPath
                    arguments = @($ackPath) + $arguments
                    working_directory = $workingDirectory
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
    $env:GRADIAL_PROCESS_SMOKE_INHERITED = $identity
    & (Join-Path $PSScriptRoot "smoke-interaction.ps1") `
        -Configuration release `
        -Geometry fixed-origin `
        -Slices 8 `
        -Direction E `
        -InvocationCount 1 `
        -ExpectProcessExecution

    $ackDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while (
        -not (Test-Path -LiteralPath $ackPath) -and
        [DateTime]::UtcNow -lt $ackDeadline
    ) {
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path -LiteralPath $ackPath)) {
        throw "The isolated executable did not acknowledge Gradial's launch."
    }

    $acknowledgement = [System.IO.File]::ReadAllLines($ackPath)
    if ($acknowledgement.Length -ne ($arguments.Count + 4)) {
        throw "The isolated executable reported an unexpected acknowledgement shape."
    }
    $receiverProcessId = [int]$acknowledgement[0]
    $receivedWorkingDirectory = ConvertFrom-Base64Utf8 $acknowledgement[1]
    if ($receivedWorkingDirectory -cne $workingDirectory) {
        throw "CreateProcessW did not apply the exact configured working directory."
    }
    $receivedInheritedValue = ConvertFrom-Base64Utf8 $acknowledgement[2]
    if ($receivedInheritedValue -cne $identity) {
        throw "The child did not inherit Gradial's normal process environment."
    }
    if ([int]$acknowledgement[3] -ne $arguments.Count) {
        throw "The isolated executable received an unexpected argument count."
    }
    for ($index = 0; $index -lt $arguments.Count; $index++) {
        $received = ConvertFrom-Base64Utf8 $acknowledgement[$index + 4]
        if ($received -cne $arguments[$index]) {
            throw "Argument $index did not survive the Windows process boundary exactly."
        }
    }

    $receiver = Get-Process -Id $receiverProcessId -ErrorAction Stop
    if ($receiver.Path -cne $receiverPath) {
        throw "The acknowledgement process did not match the isolated receiver."
    }

    $logPath = Join-Path $temporaryRoot "Gradial\logs\gradial_rCURRENT.log"
    $log = Get-Content -Raw -LiteralPath $logPath
    if ($log.Contains($privacySentinel)) {
        throw "An executable path, working directory, or argument appeared in Gradial's log."
    }
    if (
        $log -notmatch (
            "event_code=process_launch_step_started .*argument_count=" +
            ($arguments.Count + 1) + " .*working_directory_configured=true"
        )
    ) {
        throw "Gradial did not record the redacted process-launch metadata."
    }
    "CreateProcessW preserved every argument and the working directory without waiting for child exit."
}
finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    $env:GRADIAL_PROCESS_SMOKE_INHERITED = $originalInheritedValue
    $receivers = @(
        Get-Process -ErrorAction SilentlyContinue |
            Where-Object { $_.Path -ceq $receiverPath }
    )
    if ($receivers.Count -gt 1) {
        throw "More than one process matched the isolated receiver path."
    }
    if ($receivers.Count -eq 1) {
        Stop-Process -Id $receivers[0].Id -Force
        Wait-Process -Id $receivers[0].Id -ErrorAction SilentlyContinue
    }

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
        for ($attempt = 1; $attempt -le 20; $attempt++) {
            try {
                Remove-Item -LiteralPath $resolvedTemporary.Path -Recurse -Force
                break
            }
            catch {
                if ($attempt -eq 20) {
                    throw
                }
                Start-Sleep -Milliseconds 100
            }
        }
    }
}
