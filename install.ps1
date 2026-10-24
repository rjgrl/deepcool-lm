<#
.SYNOPSIS
    Installs the DeepCool LM app (cooler screen, tray icon and dashboard) on Windows 10/11.

.DESCRIPTION
    - Copies deepcool-lm.exe (built with cargo) to "C:\Program Files\deepcool-lm"
    - Adds the "deepcool-lm" command to the system PATH
    - Adds a Start menu shortcut and, if you want, starts the app when you sign in
    - Prepares the settings folder (C:\ProgramData\deepcool-lm)
    - Removes the older Python version if it's installed
    - Checks that the cooler uses the WinUSB driver and that
      LibreHardwareMonitor is available for temperatures

    Build the app first (needs Rust from https://rustup.rs), in the project folder:
        cargo build --release
    (The installer offers to run the build for you if cargo is installed.)
    Then run from an elevated PowerShell in the project folder:
        powershell -ExecutionPolicy Bypass -File .\install.ps1

.PARAMETER Yes
    Answer "yes" to every prompt (non-interactive install).

.PARAMETER StartAtBoot
    Show saved content before sign-in using a limited, password-free scheduled
    task under your Windows account. The tray app takes over at sign-in.

.PARAMETER Orientation
    How the cooler is mounted: 'landscape' (screen wider than tall) or
    'portrait' (taller than wide). Shortcut for -Rotation 0 or 90.

.PARAMETER Rotation
    Screen orientation as a clockwise turn: 0 landscape, 90 portrait,
    180 / 270 the same upside down. If neither this nor -Orientation is given,
    the installer keeps the previous choice or asks.
#>
param(
    [switch]$Yes,
    [switch]$StartAtBoot,
    [ValidateSet('landscape', 'portrait')]
    [string]$Orientation,
    [ValidateSet(0, 90, 180, 270)]
    [int]$Rotation = -1
)

$ErrorActionPreference = 'Stop'

$TaskName     = 'DeepCool LM'
$InstallDir   = Join-Path $env:ProgramFiles 'deepcool-lm'
$AppDir       = Join-Path $InstallDir 'app'        # the exe lives here, off PATH
$AppExe       = Join-Path $AppDir 'deepcool-lm.exe'
$Shim         = Join-Path $InstallDir 'deepcool-lm.cmd'
$TrayIcon     = Join-Path $InstallDir 'deepcool-lm.ico'
$DataDir      = Join-Path $env:ProgramData 'deepcool-lm'
$ConfigFile   = Join-Path $DataDir 'config.json'
$RunKeyHKLM   = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run'
$StartMenuLnk = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\DeepCool LM.lnk'
$SourceDir    = $PSScriptRoot
$BuiltExe     = Join-Path $SourceDir 'target\release\deepcool-lm.exe'

function Write-Step($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Write-Ok($msg)   { Write-Host "  [OK] $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  [!]  $msg" -ForegroundColor Yellow }
function Write-Err($msg)  { Write-Host "  [X]  $msg" -ForegroundColor Red }

function Confirm-Choice($question, [bool]$default = $true) {
    if ($Yes) { return $true }
    $hint = if ($default) { '(Y/n)' } else { '(y/N)' }
    $reply = Read-Host "$question $hint"
    if ([string]::IsNullOrWhiteSpace($reply)) { return $default }
    return $reply -match '^[Yy]'
}

Write-Host '=========================================='
Write-Host '       DeepCool LM Installer'
Write-Host '          (Windows 10/11)'
Write-Host '=========================================='
Write-Host ''

# --- Administrator check ------------------------------------------------------
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Err 'This script must be run as Administrator.'
    Write-Host '     Right-click PowerShell or Terminal > "Run as administrator", then run:'
    Write-Host '     powershell -ExecutionPolicy Bypass -File .\install.ps1'
    exit 1
}

# --- The app ---------------------------------------------------------------------
if (-not (Test-Path $BuiltExe)) {
    Write-Warn "deepcool-lm.exe hasn't been built yet ($BuiltExe)."
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargo -and (Confirm-Choice '     Build it now with cargo?')) {
        Write-Step 'Building (cargo build --release)...'
        Push-Location $SourceDir
        try { & cargo build --release } finally { Pop-Location }
    }
    if (-not (Test-Path $BuiltExe)) {
        Write-Err 'No deepcool-lm.exe to install. Install Rust from https://rustup.rs, then run'
        Write-Host '     this in the project folder:  cargo build --release'
        exit 1
    }
}
Write-Ok "Found deepcool-lm.exe ($([math]::Round((Get-Item $BuiltExe).Length / 1KB)) KB)"

# --- Stop any running copy ---------------------------------------------------------
$previousRotation = $null
if (Test-Path $ConfigFile) {
    try {
        $saved = Get-Content $ConfigFile -Raw -Encoding UTF8 | ConvertFrom-Json
        $r = if ($null -ne $saved.rotation) { [int]$saved.rotation } else { $null }
        # Older settings had a 'portrait' panel type; its equivalent is a quarter turn
        if ($saved.orientation -eq 'portrait') { $r = ((0 + $r) + 90) % 360 }
        if ($null -ne $r) { $previousRotation = $r }
    } catch { }
}
$oldTask = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
$bootTask = Get-ScheduledTask -TaskName 'DeepCool LM Boot' -TaskPath '\' -ErrorAction SilentlyContinue
$hadBootStartup = $bootTask -and $bootTask.Settings.Enabled
if ($bootTask) {
    if (@($bootTask.Actions).Count -ne 1 -or $bootTask.Actions[0].Execute -ne $AppExe -or
        $bootTask.Actions[0].Arguments -ne '--boot') {
        throw "An unexpected task already uses the name 'DeepCool LM Boot'; it was not changed."
    }
    Disable-ScheduledTask -TaskName 'DeepCool LM Boot' -TaskPath '\' | Out-Null
    Stop-ScheduledTask -TaskName 'DeepCool LM Boot' -TaskPath '\' -ErrorAction SilentlyContinue
}
if ($null -eq $previousRotation -and $oldTask) {
    $oldArgs = @($oldTask.Actions | ForEach-Object { $_.Arguments }) -join ' '
    if ($oldArgs -match '--rotation\s+(0|90|180|270)') { $previousRotation = [int]$Matches[1] }
    elseif ($oldArgs -match '--orientation\s+portrait') { $previousRotation = 90 }
}
Get-Process -Name 'deepcool-lm' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
# the older Python version (tray app or background task)
Get-CimInstance Win32_Process -Filter "Name LIKE 'python%'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -like '*deepcool-lm.py*' -or $_.CommandLine -like '*deepcool_lm*' -or
                   $_.CommandLine -like '*deepcool-lm-tray*' } |
    ForEach-Object { Invoke-CimMethod -InputObject $_ -MethodName Terminate | Out-Null }
if ($oldTask) {
    Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    Write-Ok "Removed the old '$TaskName' background task"
}
Start-Sleep -Seconds 1

# --- Leftovers from older versions -------------------------------------------------
$oldServices = @(Get-CimInstance Win32_Service -ErrorAction SilentlyContinue | Where-Object {
    $_.PathName -like "*$InstallDir*" -or $_.Name -like '*deepcool*' -or $_.DisplayName -like '*deepcool*'
})
foreach ($svc in $oldServices) {
    Write-Warn "Found an older DeepCool Windows service: '$($svc.Name)' ($($svc.State)) -> $($svc.PathName)"
    if (Confirm-Choice "     Stop and remove service '$($svc.Name)'? It conflicts with this app.") {
        Stop-Service -Name $svc.Name -Force -ErrorAction SilentlyContinue
        & sc.exe delete $svc.Name | Out-Null
        Write-Ok "Removed service '$($svc.Name)'"
    }
}
# The Python version's files, and anything that would shadow deepcool-lm.cmd on PATH
$leftovers = @('venv', 'deepcool-lm.py', 'deepcool-lm-tray.pyw', 'dashboard.html', '__pycache__',
               'deepcool-lm.bat', 'deepcool-lm.exe', 'deepcool-lm.com', 'deepcool-lm.ps1',
               'deepcool_lm_windows.py', 'deepcool-lm-service.py', 'nssm.exe', 'service*.log', 'service_error*.log')
$removedOld = $false
foreach ($pattern in $leftovers) {
    foreach ($stale in @(Get-ChildItem -Path $InstallDir -Filter $pattern -Force -ErrorAction SilentlyContinue)) {
        try {
            Remove-Item -Recurse -Force $stale.FullName
            $removedOld = $true
        } catch {
            Write-Warn "Could not remove $($stale.FullName): $($_.Exception.Message)"
        }
    }
}
if ($removedOld) { Write-Ok 'Removed files from the older Python version' }
Remove-ItemProperty -Path $RunKeyHKLM -Name 'DeepCool LM Tray' -ErrorAction SilentlyContinue
$otherTasks = @(Get-ScheduledTask -ErrorAction SilentlyContinue | Where-Object {
    $_.TaskName -notin @($TaskName, 'DeepCool LM Boot') -and
    (@($_.Actions | ForEach-Object { "$($_.Execute) $($_.Arguments)" }) -join ' ') -like '*deepcool*'
})
foreach ($t in $otherTasks) {
    Write-Warn "Another scheduled task runs a DeepCool program: '$($t.TaskPath)$($t.TaskName)' ($($t.State))."
    Write-Host '     It can fight this app for the screen. Disable it with:'
    Write-Host "     Disable-ScheduledTask -TaskName '$($t.TaskName)' -TaskPath '$($t.TaskPath)'"
}

# --- Files -------------------------------------------------------------------------
Write-Step "Installing to $InstallDir..."
New-Item -ItemType Directory -Force -Path $InstallDir, $AppDir, $DataDir, (Join-Path $DataDir 'media') | Out-Null
Copy-Item -Force $BuiltExe $AppExe
# The exe is a windowed program; going through a .cmd makes terminals wait for its output
$shimText = "@echo off`r`n`"%~dp0app\deepcool-lm.exe`" %*`r`n"
[IO.File]::WriteAllText($Shim, $shimText, (New-Object Text.ASCIIEncoding))
& $Shim --write-icon $TrayIcon | Out-Null
Write-Ok "Installed deepcool-lm.exe and the deepcool-lm command"

# Settings folder: the app (your account) writes settings and uploaded images there
& icacls.exe $DataDir /grant '*S-1-5-32-545:(OI)(CI)M' /T /Q | Out-Null
if ($LASTEXITCODE -eq 0) { Write-Ok "Settings folder ready: $DataDir" }
else { Write-Warn "Could not set permissions on $DataDir; the dashboard may not be able to save." }

# --- PATH -------------------------------------------------------------------------
$envKey = 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
$machinePath = (Get-Item $envKey).GetValue('Path', '', 'DoNotExpandEnvironmentNames')
$parts = @($machinePath -split ';' | Where-Object { $_ -ne '' })
if ($parts -notcontains $InstallDir) {
    # Write as REG_EXPAND_SZ so existing %SystemRoot%-style entries keep working
    Set-ItemProperty -Path $envKey -Name 'Path' -Value (($parts + $InstallDir) -join ';') -Type ExpandString
    # Tell Explorer so newly opened terminals see the change
    Add-Type -Namespace DeepcoolLM -Name Native -MemberDefinition @'
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
'@
    $result = [UIntPtr]::Zero
    [DeepcoolLM.Native]::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, 'Environment', 2, 5000, [ref]$result) | Out-Null
    Write-Ok 'Added to system PATH (open a new terminal to use "deepcool-lm")'
}

# --- Device + driver check -------------------------------------------------------
Write-Step 'Checking for the cooler (USB 3633:0026)...'
$devices = @(Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue |
             Where-Object { $_.InstanceId -like 'USB\VID_3633&PID_0026*' })
if ($devices.Count -eq 0) {
    Write-Warn 'Cooler not detected. Make sure its USB cable is connected to a motherboard header.'
} else {
    $driverOk = $false
    foreach ($d in $devices) {
        $svc = (Get-PnpDeviceProperty -InstanceId $d.InstanceId -KeyName 'DEVPKEY_Device_Service' -ErrorAction SilentlyContinue).Data
        Write-Host "     Found: $($d.FriendlyName)  [driver: $svc]"
        if ($svc -eq 'WinUSB') { $driverOk = $true }
    }
    if ($driverOk) {
        Write-Ok 'The cooler uses the WinUSB driver'
    } else {
        Write-Warn 'The cooler is not using the WinUSB driver, so the app cannot talk to it.'
        Write-Host '     It normally gets Windows'' own WinUSB driver automatically.'
        Write-Host '     Check: pnputil /enum-devices /deviceid "USB\VID_3633&PID_0026" /drivers'
        Write-Host '     See README.md ("Check the USB driver") for how to get back to winusb.inf.'
    }
    $thirdParty = @(pnputil /enum-devices /deviceid 'USB\VID_3633&PID_0026' /drivers 2>$null |
                    Select-String -Pattern 'Provider Name:\s+(libusbK|libwdi)')
    if ($thirdParty.Count -gt 0) {
        Write-Warn 'Zadig/libwdi driver packages are installed for the cooler. They hide it from'
        Write-Host '     WinUSB and from DeepCool''s app. README.md ("Check the USB driver") shows how'
        Write-Host '     to remove them so Windows uses its own WinUSB driver.'
    }
}

# --- Temperature source check ---------------------------------------------------
Write-Step 'Checking for a temperature source...'
$lhmUrl = if ($env:DEEPCOOL_LM_LHM_URL) { $env:DEEPCOOL_LM_LHM_URL } else { 'http://127.0.0.1:8085/data.json' }
$lhmProcess = Get-Process -Name 'LibreHardwareMonitor', 'OpenHardwareMonitor' -ErrorAction SilentlyContinue
$lhmWeb = $false
try {
    $null = Invoke-RestMethod -Uri $lhmUrl -TimeoutSec 3 -UseBasicParsing
    $lhmWeb = $true
} catch { }
if ($lhmWeb) {
    Write-Ok "LibreHardwareMonitor web server is answering at $lhmUrl - temperatures available"
} elseif ($lhmProcess) {
    Write-Warn 'LibreHardwareMonitor is running, but its web server is off.'
    Write-Host '     In LibreHardwareMonitor: Options > Remote Web Server > Run   (default port 8085)'
    Write-Host '     The app picks it up automatically once it is on.'
} else {
    Write-Warn 'LibreHardwareMonitor is not running. Windows has no built-in temperature API,'
    Write-Host '     so CPU temperature will show 0 until it is. Get it from:'
    Write-Host '     https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases'
    Write-Host '     Run it as administrator, then enable Options > Remote Web Server > Run'
    Write-Host '     and Options > Run On Windows Startup.'
    Write-Host '     (NVIDIA GPU temperature works without it.)'
}

# --- Screen orientation ----------------------------------------------------------
Write-Host ''
$rotationNames = @{ 0 = 'landscape'; 90 = 'portrait'; 180 = 'landscape, upside down'; 270 = 'portrait, upside down' }
if ($Rotation -lt 0) {
    if ($Orientation) {
        $Rotation = if ($Orientation -eq 'portrait') { 90 } else { 0 }
    } elseif ($null -ne $previousRotation) {
        $Rotation = $previousRotation
        Write-Ok "Keeping screen orientation: $($rotationNames[$Rotation]) ($Rotation degrees)"
    } elseif ($Yes) {
        $Rotation = 0
    } else {
        Write-Host 'Screen orientation: as mounted, is the cooler''s screen wider than tall (landscape)'
        Write-Host 'or taller than wide (portrait)? You can change this any time from the tray icon.'
        $reply = Read-Host 'Orientation [L]andscape / [P]ortrait (L)'
        $Rotation = if ($reply -match '^[Pp]') { 90 } else { 0 }
    }
}
& $Shim settings --set-rotation $Rotation | Out-Null

# --- Start menu + sign-in ------------------------------------------------------------
Write-Step 'Setting up the DeepCool LM app...'
$shell = New-Object -ComObject WScript.Shell
$lnk = $shell.CreateShortcut($StartMenuLnk)
$lnk.TargetPath = $AppExe
$lnk.Arguments = '--open'
$lnk.WorkingDirectory = $AppDir
$lnk.Description = 'DeepCool LM cooler screen'
if (Test-Path $TrayIcon) { $lnk.IconLocation = $TrayIcon }
$lnk.Save()
Write-Ok 'Start menu shortcut: DeepCool LM'
# Your own sign-in entry; the app's tray menu can turn it on and off too
if ($StartAtBoot -or $hadBootStartup) {
    & (Join-Path $PSScriptRoot 'enable-boot-start.ps1')
    Write-Ok 'Boot display enabled; the tray app takes over at sign-in'
} elseif (Confirm-Choice 'Start DeepCool LM when you sign in? (the cooler screen runs while the app is open)') {
    & $Shim autostart on | Out-Null
    Write-Ok 'DeepCool LM will start when you sign in'
} else {
    & $Shim autostart off | Out-Null
}
# Launch through Explorer so the app runs as you, not as administrator
Start-Process -FilePath 'explorer.exe' -ArgumentList "`"$StartMenuLnk`""
Write-Ok 'DeepCool LM started - look for its icon near the clock'

$TrayLog = Join-Path $env:LOCALAPPDATA 'deepcool-lm\tray.log'
Write-Host ''
Write-Host '=========================================='
Write-Host '  Installation Complete!'
Write-Host '=========================================='
Write-Host ''
Write-Host 'DeepCool LM runs the cooler screen while its tray icon is running.'
Write-Host 'Click the icon (or open "DeepCool LM" from the Start menu) for the dashboard.'
Write-Host "Log: $TrayLog"
Write-Host ''
Write-Host 'Commands (any new terminal, no admin needed; they talk to the running app):'
Write-Host '  Display image:     deepcool-lm image C:\path\to\image.jpg'
Write-Host '  Display color:     deepcool-lm solid --color 255 0 0'
Write-Host '  Brightness:        deepcool-lm brightness 70'
Write-Host '  Back to monitor:   deepcool-lm monitor'
Write-Host '  Saved settings:    deepcool-lm settings'
Write-Host ''
Write-Host "Run 'deepcool-lm --help' for more options"
Write-Host ''
