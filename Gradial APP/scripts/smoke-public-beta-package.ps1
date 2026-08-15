param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [string]$ExpectedVersion = "0.3.0-beta.1"
)

$ErrorActionPreference = "Stop"

function Invoke-Installer([string[]]$Arguments, [string]$Operation) {
    $process = Start-Process -FilePath $resolvedInstaller -ArgumentList $Arguments -PassThru -Wait
    if ($process.ExitCode -ne 0) {
        throw "$Operation failed with exit code $($process.ExitCode)."
    }
}

function Wait-ForReadyLog([string]$Path, [System.Diagnostics.Process]$Process) {
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($Process.HasExited) {
            $details = ""
            if (Test-Path -LiteralPath $standardErrorPath) {
                $details = (Get-Content -Raw -LiteralPath $standardErrorPath).Trim()
            }
            throw "The installed Gradial process exited before startup completed (exit code $($Process.ExitCode)). $details"
        }
        if (Test-Path -LiteralPath $Path) {
            $contents = Get-Content -Raw -LiteralPath $Path
            if ($contents -match "event_code=overlay_pool_ready result_code=ok") {
                return
            }
        }
        Start-Sleep -Milliseconds 100
    }
    if (Test-Path -LiteralPath $Path) {
        $tail = (Get-Content -LiteralPath $Path -Tail 30) -join [Environment]::NewLine
        throw "The installed Gradial build did not report a ready overlay/input state. Recent log:`n$tail"
    }
    throw "The installed Gradial build did not create its isolated startup log at '$Path'."
}

$resolvedInstaller = (Resolve-Path -LiteralPath $InstallerPath).Path
$temporaryBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$temporaryRoot = Join-Path $temporaryBase "gradial-public-beta-smoke-$([Guid]::NewGuid().ToString('N'))"
$installRoot = Join-Path $temporaryRoot "installed"
$profileRoot = Join-Path $temporaryRoot "profile"
$logPath = Join-Path $profileRoot "Gradial\logs\gradial_rCURRENT.log"
$sentinelPath = Join-Path $profileRoot "Gradial\preserve-on-upgrade.txt"
$installLog = Join-Path $temporaryRoot "install.log"
$repairLog = Join-Path $temporaryRoot "repair.log"
$uninstallLog = Join-Path $temporaryRoot "uninstall.log"
$standardOutputPath = Join-Path $temporaryRoot "gradial.stdout.log"
$standardErrorPath = Join-Path $temporaryRoot "gradial.stderr.log"
$originalLocalAppData = $env:LOCALAPPDATA
$gradial = $null

$otherGradial = @(Get-Process -Name "gradial" -ErrorAction SilentlyContinue)
if ($otherGradial.Count -ne 0) {
    throw "Close the running Gradial instance before the package smoke test."
}

try {
    New-Item -ItemType Directory -Path $profileRoot -Force | Out-Null
    Invoke-Installer @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/NOICONS",
        "/DIR=`"$installRoot`"",
        "/LOG=`"$installLog`""
    ) "Clean install"

    $installedExecutable = Join-Path $installRoot "gradial.exe"
    if (-not (Test-Path -LiteralPath $installedExecutable)) {
        throw "The installer did not create gradial.exe."
    }
    $installedVersion = (Get-Item -LiteralPath $installedExecutable).VersionInfo.ProductVersion
    if ($installedVersion -cne $ExpectedVersion) {
        throw "Installed product version '$installedVersion' did not match '$ExpectedVersion'."
    }

    $env:LOCALAPPDATA = $profileRoot
    $gradial = Start-Process `
        -FilePath $installedExecutable `
        -WorkingDirectory $installRoot `
        -WindowStyle Hidden `
        -RedirectStandardOutput $standardOutputPath `
        -RedirectStandardError $standardErrorPath `
        -PassThru
    $env:LOCALAPPDATA = $originalLocalAppData
    Wait-ForReadyLog $logPath $gradial

    if ($gradial.HasExited) {
        throw "The installed Gradial process exited during the launch smoke test."
    }
    Stop-Process -Id $gradial.Id -Force
    Wait-Process -Id $gradial.Id -ErrorAction SilentlyContinue
    $gradial = $null

    [System.IO.File]::WriteAllText(
        $sentinelPath,
        "Gradial must preserve this local state across an in-place install.",
        [System.Text.UTF8Encoding]::new($false)
    )
    Invoke-Installer @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/NOICONS",
        "/DIR=`"$installRoot`"",
        "/LOG=`"$repairLog`""
    ) "In-place reinstall"
    if (-not (Test-Path -LiteralPath $sentinelPath)) {
        throw "The in-place installer removed user data."
    }

    $uninstaller = Join-Path $installRoot "unins000.exe"
    if (-not (Test-Path -LiteralPath $uninstaller)) {
        throw "The installer did not create an uninstaller."
    }
    $uninstall = Start-Process -FilePath $uninstaller -ArgumentList @(
        "/VERYSILENT",
        "/SUPPRESSMSGBOXES",
        "/NORESTART",
        "/LOG=`"$uninstallLog`""
    ) -PassThru -Wait
    if ($uninstall.ExitCode -ne 0) {
        throw "Uninstall failed with exit code $($uninstall.ExitCode)."
    }
    if (Test-Path -LiteralPath $installedExecutable) {
        throw "Uninstall left the application executable behind."
    }
    if (-not (Test-Path -LiteralPath $sentinelPath)) {
        throw "Uninstall removed user data that should be preserved."
    }

    [pscustomobject]@{
        result = "passed"
        version = $ExpectedVersion
        clean_install = "passed"
        launch_and_health = "passed"
        in_place_reinstall = "passed"
        user_data_preserved = "passed"
        uninstall = "passed"
    } | Format-List
}
finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    if ($null -ne $gradial -and -not $gradial.HasExited) {
        Stop-Process -Id $gradial.Id -Force
        Wait-Process -Id $gradial.Id -ErrorAction SilentlyContinue
    }

    if (Test-Path -LiteralPath $temporaryRoot) {
        $resolvedTemporary = [System.IO.Path]::GetFullPath($temporaryRoot)
        if (
            -not $resolvedTemporary.StartsWith($temporaryBase) -or
            (Split-Path -Leaf $resolvedTemporary) -notlike "gradial-public-beta-smoke-*"
        ) {
            throw "Refusing to remove an unexpected package-smoke directory."
        }
        Remove-Item -LiteralPath $resolvedTemporary -Recurse -Force
    }
}
