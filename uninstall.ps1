<#
.SYNOPSIS
    Removes the DeepCool LM app from Windows.

    Run from an elevated PowerShell:
        powershell -ExecutionPolicy Bypass -File .\uninstall.ps1
#>
$ErrorActionPreference = 'Stop'

$TaskName   = 'DeepCool LM'
$InstallDir = Join-Path $env:ProgramFiles 'deepcool-lm'
$DataDir    = Join-Path $env:ProgramData 'deepcool-lm'

function Write-Ok($msg)  { Write-Host "  [OK] $msg" -ForegroundColor Green }
function Write-Err($msg) { Write-Host "  [X]  $msg" -ForegroundColor Red }

Write-Host '=========================================='
Write-Host '       DeepCool LM Uninstaller'
Write-Host '=========================================='
Write-Host ''

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Err 'This script must be run as Administrator.'
    Write-Host '     Please run: powershell -ExecutionPolicy Bypass -File .\uninstall.ps1'
    exit 1
}

# Stop and remove the scheduled task
foreach ($name in @($TaskName, 'DeepCool LM Boot')) {
    if (Get-ScheduledTask -TaskName $name -TaskPath '\' -ErrorAction SilentlyContinue) {
        Stop-ScheduledTask -TaskName $name -TaskPath '\' -ErrorAction SilentlyContinue
        Unregister-ScheduledTask -TaskName $name -TaskPath '\' -Confirm:$false
        Write-Ok "Removed scheduled task: $name"
    }
}

# Tray app: autostart entry and Start menu shortcut
Remove-ItemProperty -Path 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'DeepCool LM Tray' -ErrorAction SilentlyContinue
Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'DeepCool LM' -ErrorAction SilentlyContinue
$lnk = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\DeepCool LM.lnk'
if (Test-Path $lnk) { Remove-Item -Force $lnk; Write-Ok 'Removed Start menu shortcut' }

# Make sure no copy (service or tray) is still running
Get-CimInstance Win32_Process -Filter "Name LIKE 'python%'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -like '*deepcool-lm.py*' -or $_.CommandLine -like '*deepcool-lm-tray*' } |
    ForEach-Object { Invoke-CimMethod -InputObject $_ -MethodName Terminate | Out-Null }
Get-Process -Name 'deepcool-lm' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

# Remove from the system PATH
$envKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
$machinePath = (Get-Item $envKey).GetValue('Path', '', 'DoNotExpandEnvironmentNames')
$parts = @($machinePath -split ';' | Where-Object { $_ -ne '' })
if ($parts -contains $InstallDir) {
    $kept = @($parts | Where-Object { $_ -ne $InstallDir })
    Set-ItemProperty -Path $envKey -Name 'Path' -Value ($kept -join ';') -Type ExpandString
    Write-Ok 'Removed from system PATH'
}

# Remove program files and logs
if (Test-Path $InstallDir) {
    Remove-Item -Recurse -Force $InstallDir
    Write-Ok "Removed $InstallDir"
}
if (Test-Path $DataDir) {
    Remove-Item -Recurse -Force $DataDir
    Write-Ok "Removed settings, uploaded images and logs in $DataDir"
}

Write-Host ''
Write-Host '=========================================='
Write-Host '  Uninstallation Complete!'
Write-Host '=========================================='
Write-Host ''
Write-Host 'The cooler keeps using Windows'' own WinUSB driver, so DeepCool''s app can'
Write-Host 'use it again straight away.'
Write-Host ''
