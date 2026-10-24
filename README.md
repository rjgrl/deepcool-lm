# DeepCool LM

A small Windows 10/11 app for the LCD on DeepCool LM-series AIO coolers (320×240 screen). It shows live CPU and GPU readings in a layout you design, or your own pictures and GIFs. Everything is one `deepcool-lm.exe` that uses about as much memory as a tray icon.

![Resolution](https://img.shields.io/badge/Resolution-320x240-blue) ![License](https://img.shields.io/badge/License-MIT-green)

![DeepCool LM360 screen](lm360.png)

> Tested on the **LM360**. Other LM models with USB ID `3633:0026` (LM240, LM280, ...) will probably work but haven't been tested. Reports are welcome.

## Features

- **Layout editor**: drag Text, Bar, Gauge, Graph and Panel widgets onto a picture of the pump cap. Each one can show any sensor LibreHardwareMonitor reports (temperatures, clocks, loads, fans, power), plus RAM, disk and network.
- **Fonts**: any font installed on your PC, with bold and italic, for every text widget and gauge value
- **Backgrounds**: a color, or a picture or animated GIF with its own fit and brightness
- **Images mode**: a slideshow of your pictures and GIFs
- **Any mounting**: rotate the picture in quarter turns to match how the cooler is mounted
- **Brightness** slider, and a **test pattern** for checking colors and orientation
- **Tray icon** with quick settings; changes apply instantly
- **Command line**: `deepcool-lm brightness 70`, `deepcool-lm image photo.png` and more
- **Light on resources**: no Python, no runtime, no third-party libraries. It uses only Windows itself (WinUSB, GDI+), and long GIFs are decoded frame by frame.

## Install

### 1. Build the app

Install Rust from [rustup.rs](https://rustup.rs) (accept the default *Visual Studio Build Tools* option when it offers it). Then, in the project folder:

```powershell
cargo build --release
```

Nothing is downloaded besides Rust itself. The app is `target\release\deepcool-lm.exe`.

### 2. Check the USB driver (normally nothing to do)

The cooler asks Windows for Microsoft's built-in **WinUSB** driver by itself (it matches `USB\MS_COMP_WINUSB`). That's the same driver DeepCool's app uses. In Device Manager it shows as **LM-Series** under **Universal Serial Bus devices**. The installer checks this for you.

**Don't use Zadig** (or other libusb driver installers) on this cooler. Zadig installs its own generated copy of the driver, which DeepCool's app can't see, and Windows keeps reusing it. If you already did, remove those packages:

```powershell
pnputil /enum-devices /deviceid "USB\VID_3633&PID_0026" /drivers
```

For every matching driver whose provider is **libusbK** or **libwdi**, run `pnputil /delete-driver oemNN.inf /uninstall /force`. Then use Device Manager → **Action → Scan for hardware changes**. Afterwards the only matching driver should be Microsoft's `winusb.inf`.

### 3. Temperatures: LibreHardwareMonitor

Windows has no built-in API for CPU temperatures, so the app reads them from [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor/releases):

- Run it **as administrator** (needed to read CPU sensors).
- Turn on **Options → Remote Web Server → Run**. The app reads `http://127.0.0.1:8085/data.json`. If you changed the port, set the `DEEPCOOL_LM_LHM_URL` environment variable to match.
- Turn on **Options → Run On Windows Startup** (and optionally *Start Minimized* / *Minimize To Tray*).

Without it, CPU temperature shows 0°. NVIDIA GPU temperature and load still work through the NVIDIA driver.

> LibreHardwareMonitor's web server listens on all network interfaces by default, so other devices on your network could read your sensor values. When Windows Firewall asks, blocking it is fine: the app connects locally.

### 4. Install

In PowerShell or Terminal **as administrator**, in the project folder:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

The installer:
- copies the app to `C:\Program Files\deepcool-lm` and adds the `deepcool-lm` command to PATH
- adds a **DeepCool LM** Start menu shortcut
- asks whether to start the app when you sign in
- prepares the settings folder `C:\ProgramData\deepcool-lm`
- removes the older Python version of this project if it finds it
- checks the USB driver and LibreHardwareMonitor and tells you what's missing

If the app isn't built yet and Rust is installed, the installer offers to build it. Options: `-Yes` accepts every prompt, and `-Rotation 0|90|180|270` (or `-Orientation landscape|portrait`) sets the screen orientation.

### Start the LCD before sign-in

Add `-StartAtBoot` when installing, from an administrator PowerShell using your normal Windows account:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1 -StartAtBoot
```

For an already installed, updated executable, run `powershell -ExecutionPolicy Bypass -File .\enable-boot-start.ps1` instead. The **DeepCool LM Boot** scheduled task runs `--boot` after Windows starts, without waiting for sign-in. It displays your saved media or layout and waits for USB if necessary. It has no tray icon, dashboard or command server. At sign-in, the normal app asks it to finish, waits for it to release the cooler without blanking it, then takes over. A late boot task skips the screen while the normal app is running.

The task uses your account with limited privileges and S4U logon: no Windows password is stored. Network shares and EFS-encrypted media are unavailable before sign-in. CPU temperatures appear once LibreHardwareMonitor starts; this option does not start LibreHardwareMonitor. GDI+ rendering in this noninteractive task must be verified on the actual PC. It is a scheduled application, not a Windows service.

This starts during **Windows boot**, not BIOS/POST, and cannot guarantee recovery from a cooler that needs a physical power reset. Boot logs are in `C:\ProgramData\deepcool-lm\boot.log`. To disable boot startup, disable **DeepCool LM Boot** in Task Scheduler; sign-in startup still works. To stop an active boot worker gracefully, start the normal app. Turning off **Start when I sign in** affects the tray app only, so leave it enabled when using boot startup.

### Without the installer

`deepcool-lm.exe` also works on its own: copy it anywhere and run it. The dashboard is built in and it needs no other files. On first save it creates the settings folder in `C:\ProgramData\deepcool-lm`. To start it when you sign in, right-click its tray icon and pick **Start when I sign in** (or run `deepcool-lm autostart on`). If you move the exe later, start it once from its new place and the sign-in entry follows it.

What the installer adds on top: the `deepcool-lm` command on PATH, the Start menu shortcut, and the USB and temperature checks.

## Using it

### Tray icon and dashboard

Look for the **DeepCool LM** icon near the clock. Click it to open the dashboard. Right-click it to:
- switch between Monitor and Images mode
- change orientation and brightness
- show a test pattern
- turn **Start when I sign in** on or off
- quit

The app *is* the screen driver. It runs the screen from when it starts until you quit, waits for the cooler if it isn't ready, and reconnects when it's unplugged. **Quit** turns the screen off and closes the dashboard. By default it starts at sign-in; the optional boot task above can display your content before sign-in.

The dashboard has three sections:

- **Display**: Monitor or Images mode, screen orientation, refresh rate, brightness and the test pattern.
- **Layout**: the editor. Add widgets, drag them into place, pull a corner to resize, and pick a sensor, colors and font for each. Landscape and portrait have separate layouts. Undo, redo and keyboard nudging work as you'd expect.
- **Images**: drop in PNG, JPG, GIF or BMP files. Transparent parts show the background color you pick. Choose which images to show, their order, how they fit, and how long each stays up.

The preview uses the same drawing code as the cooler. **Save to cooler** applies the changes straight away and stores them in `C:\ProgramData\deepcool-lm\config.json`.

Text widgets understand placeholders:
- `{value}` (the widget's own sensor), `{unit}` and `{name}`
- `{time}`, `{seconds}`, `{date}` and `{day}`
- any other sensor by its id, with optional decimals, e.g. `Usage: {value}% at {cpu.clock|2} GHz`

**Fonts**: each text widget (and each gauge's value) can use any font installed on your PC, in bold, italic or both. Arrows and symbols such as ⚙ ▣ ▲ always come from Segoe UI Symbol, so they show up whatever font you choose. Fonts you install while the app is running appear in the list after you restart the app.

The dashboard opens in an Edge app window with its own small profile, so it doesn't touch your normal Edge windows. It's served by the app on a random local port and only answers requests carrying a per-session key.

### Screen orientation

| Setting | Rotation | Picture |
|---|---|---|
| Landscape | 0° | wider than tall |
| Portrait | 90° (clockwise) | taller than wide |
| Landscape, upside down | 180° | |
| Portrait, upside down | 270° | |

Change it from the dashboard, the tray menu, or `deepcool-lm settings --set-rotation 90`. The editor always shows the picture upright.

### Brightness

Brightness is a level from 0 to 100. The dashboard slider changes it live, and **Save to cooler** stores it so the app sets it every time it starts. In a terminal, run `deepcool-lm brightness 70`. `up` and `down` send the levels 97 and 29, which are the "brighter" and "dimmer" commands DeepCool's own software uses.

### Command line

From any new terminal, without admin rights:

```powershell
deepcool-lm image C:\Users\me\Pictures\wallpaper.png
deepcool-lm solid --color 255 0 0
deepcool-lm monitor              # back to the live screen
deepcool-lm brightness 70        # 0-100, or up / down
deepcool-lm zen                  # toggle the screen on/off
deepcool-lm test-pattern 10      # color bars for 10 seconds
deepcool-lm settings             # show saved settings
deepcool-lm settings --set-rotation 90 --brightness 70 --mode monitor
deepcool-lm autostart on|off     # start when you sign in
deepcool-lm info                 # USB details (quit the app first)
deepcool-lm --help
```

Commands go to the running app. When it isn't running, they talk to the cooler directly. If port 36330 is taken by another program, set `DEEPCOOL_LM_PORT` (for your account, so the app and the command line both see it).

### Files

| What | Where |
|---|---|
| App | `C:\Program Files\deepcool-lm\app\deepcool-lm.exe` |
| Settings and uploaded images | `C:\ProgramData\deepcool-lm\` |
| Log | `%LOCALAPPDATA%\deepcool-lm\tray.log` |
| Boot log (optional) | `C:\ProgramData\deepcool-lm\boot.log` |
| Start before sign-in (optional) | Task Scheduler → **DeepCool LM Boot** |
| Start at sign-in | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `DeepCool LM` |

## Uninstall

```powershell
powershell -ExecutionPolicy Bypass -File .\uninstall.ps1
```

This removes the app, the sign-in entry, the Start menu shortcut, the PATH entry, and the settings folder with your layouts and images. The cooler keeps using Windows' own WinUSB driver, so DeepCool's app works again straight away.

## Troubleshooting

- **"Failed to connect to LM360 (device not found)"**: check Device Manager for `USB\VID_3633&PID_0026`, and that the cooler's USB cable is on an internal USB header.
- **"Access denied"**: another program has the cooler open, usually DeepCool's software or a second copy of this app. Only one program can use it at a time.
- **"The device isn't using the WinUSB driver"**: see [Check the USB driver](#2-check-the-usb-driver-normally-nothing-to-do).
- **Picture sideways or upside down**: change the screen orientation.
- **Colors look wrong**: use **Show test pattern** and compare the cooler with the preview.
- **Screen stays dark but the log shows frames**: the LCD is switched off inside the cooler, which remembers that across reboots. Quit DeepCool LM, open DeepCool's app, turn on its **Zen mode** (the screen lights up), close DeepCool's app completely, and start DeepCool LM again. `deepcool-lm zen` toggles the same setting.
- **CPU shows 0°**: LibreHardwareMonitor isn't running as administrator, or its web server is off. Open `http://127.0.0.1:8085/data.json` in a browser to check.
- **Anything else**: look at `%LOCALAPPDATA%\deepcool-lm\tray.log`.

## Development

```powershell
cargo build --release     # the app
cargo test                # unit tests
```

The code uses only the Rust standard library. Windows APIs are declared in `src/win/ffi.rs` and linked with `raw-dylib`, so no Windows SDK import libraries are needed.

| File | What it does |
|---|---|
| `src/main.rs` | command line and startup |
| `src/engine.rs` | the display loop: sensors, drawing, sending, reconnecting |
| `src/render.rs` | widget drawing |
| `src/media.rs` | images and GIFs |
| `src/http.rs`, `src/dashboard.html` | the dashboard and its local API |
| `src/ipc.rs` | the command-line link to the running app |
| `src/sensors.rs` | LibreHardwareMonitor, CPU, RAM, disk, network, NVIDIA |
| `src/win/` | WinUSB, GDI+, tray icon, system info |

The `src/platform/unix.rs` stand-ins exist only for development and tests on other systems. They include a fake cooler: set `DEEPCOOL_LM_FAKE_DEVICE=frame.bin`, and `RUSTFLAGS="--cfg wincheck" cargo check` type-checks the Windows code there.

### Protocol

- USB `3633:0026`, bulk endpoint `0x01` OUT (`0x81` IN for replies)
- Packets: `0xAA`, payload length (LE16), payload, CRC-16/XMODEM of the payload (LE16)
- Frame: header packet `aa 08 00 00 01 00 58 02 00 2c 01 bc 11`, then 153,600 bytes of 320×240 RGB565 little-endian
- Init `09`, brightness `06 03 <level> 00`, Zen toggle `03 00 00 00`

## Credits

The USB protocol was worked out by reverse engineering DeepCool's official software, in the original Linux driver this project grew from. This version is a Windows-only rewrite in Rust.

## License

MIT
