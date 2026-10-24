<#
.SYNOPSIS
    Starts the installed DeepCool LM display before Windows sign-in.
.DESCRIPTION
    Run as administrator using the same Windows account that normally runs the app.
    The boot task runs with limited privileges and stores no password. It yields
    gracefully to the sign-in app. Network shares and EFS-encrypted media are not
    available to this S4U task.
#>
$ErrorActionPreference = 'Stop'
$taskIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
$taskPrincipal = [Security.Principal.WindowsPrincipal]$taskIdentity
if (-not $taskPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script as administrator, using your normal Windows account.'
}

$taskName = 'DeepCool LM Boot'
$taskApp = Join-Path $env:ProgramFiles 'deepcool-lm\app\deepcool-lm.exe'
if (-not (Test-Path -LiteralPath $taskApp -PathType Leaf)) {
    throw 'Install DeepCool LM first with install.ps1 -StartAtBoot.'
}
$taskHelp = (& $taskApp --help | Out-String)
if ($taskHelp -notmatch '(?m)^\s+deepcool-lm --boot\s') {
    throw 'The installed executable does not support boot mode. Build and install the updated app first.'
}
$taskExisting = Get-ScheduledTask -TaskName $taskName -TaskPath '\' -ErrorAction SilentlyContinue
if ($taskExisting -and (@($taskExisting.Actions).Count -ne 1 -or
    $taskExisting.Actions[0].Execute -ne $taskApp -or $taskExisting.Actions[0].Arguments -ne '--boot')) {
    throw "An unexpected task already uses the name '$taskName'; it was not changed."
}

# Handover depends on the normal app starting after sign-in.
& $taskApp autostart on | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not enable the sign-in app; boot task was not registered.' }
$taskAction = New-ScheduledTaskAction -Execute $taskApp -Argument '--boot' -WorkingDirectory (Split-Path -Parent $taskApp)
$taskTrigger = New-ScheduledTaskTrigger -AtStartup
$taskUser = New-ScheduledTaskPrincipal -UserId $taskIdentity.Name -LogonType S4U -RunLevel Limited
$taskSettings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew `
    -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -StartWhenAvailable `
    -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $taskName -TaskPath '\' -Action $taskAction -Trigger $taskTrigger `
    -Principal $taskUser -Settings $taskSettings -Description 'Display the saved DeepCool LM content before sign-in; yield to the tray app at sign-in.' -Force | Out-Null
Write-Host 'DeepCool LM will display your saved content after Windows starts, before sign-in.'
Write-Host 'Boot log: C:\ProgramData\deepcool-lm\boot.log'
Write-Host 'The tray app takes over when you sign in. No Windows password is stored.'
