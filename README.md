# Flux

> A focused desktop console for operating an Art-Net → DMX bridge.

Flux receives Art-Net `ArtDmx` and continuously drives an ENTTEC Open DMX USB interface through FTDI D2XX. It is deliberately built around one dependable local route: a lighting technician should be able to see the bridge, diagnose an interruption, adjust its configuration, and keep the show running without navigating a generic lighting framework.

```
MagicQ or another Art-Net sender
             |
             | Art-Net / ArtDmx (UDP 6454)
             v
           Flux
             |
             | FTDI D2XX
             v
     ENTTEC Open DMX USB
             |
             | DMX512
             v
          Fixtures
```

## Operate the bridge

Flux opens as an operational route console, not a collection of disconnected dashboards.

- The main view keeps the Art-Net → DMX route, input state, output state, device, universe, and traffic indicators visible at once.
- The output inspector exposes the live device state, a recovery action when output needs attention, and the on-demand DMX channel monitor.
- The title strip contains the bridge condition, app controls, and native minimize, maximize/restore, and close-to-tray actions. Drag any non-interactive area of the strip to move the window.
- Open **Configure route** from the title strip to change the listen address, universe, channel count, refresh rate, or selected FTDI device. The modal validates edits, refreshes device detection, and applies settings without restarting Flux.
- The workspace owns its scroll region; the title strip stays in place and the route surface never sits beneath a window-level scrollbar.

Closing the window hides Flux to the system tray while the Art-Net-to-DMX runtime keeps running. Reopen it from **Open Flux** in the tray menu; use **Quit Flux** only when you want an orderly shutdown.

## Get started with MagicQ

In MagicQ, open:

`SETUP` → `VIEW SETTINGS` → `Network`

Set **Net Host Options** to **Normal + Loopback IP**. Then open:

`SETUP` → `VIEW DMX I/O`

Configure Universe 1 as:

| Setting | Value |
| --- | --- |
| Status | Enabled |
| Out Type | Art-Net |
| Out Uni | 0 |

The route is then:

```
MagicQ U1 → Art-Net Universe 0 → 127.0.0.1:6454 → Flux → ENTTEC Open
```

Connect the ENTTEC Open DMX USB, ensure no other program has the device open, and start Flux:

```shell
flux
```

Flux defaults to:

| Setting | Default |
| --- | --- |
| Listen address | `127.0.0.1:6454` |
| Art-Net Port-Address | `0` |
| DMX channels | `512` |
| Refresh rate | `30 Hz` |

For an Art-Net sender on the LAN, listen on all IPv4 interfaces:

```shell
flux --listen 0.0.0.0:6454
```

## Headless and diagnostics

The desktop route console is the default. Use the CLI when running Flux on a terminal-only system, diagnosing a route, or selecting a device explicitly.

| Command | Use |
| --- | --- |
| `flux --headless` | Run without the Tauri desktop UI. |
| `flux --list-devices` | List detected FTDI devices and exit. |
| `flux --device FTXXXXXXXX` | Select an FTDI device by serial number. Required when more than one is connected. |
| `flux --dry-run -v` | Validate and inspect Art-Net without opening an FTDI device. |
| `flux --universe 0 --channels 256 --fps 30` | Override the route settings for this session. |

Use `flux --help` for the complete option reference and `flux --version` for the installed version. Repeating `-v` increases diagnostic detail.

The route configuration is stored locally after it is applied from the desktop UI. Saved settings are used on later launches whenever configuration options are not passed on the command line.

On Windows, release builds open the desktop console without creating a terminal window. Starting `flux.exe` from PowerShell with CLI options, `--headless`, or `--list-devices` retains terminal output.

## Reliable operation and hardware limits

Flux begins physical output only after receiving a valid Art-Net frame, then retains the last valid look if Art-Net disappears. If the FTDI device is disconnected or output fails, Flux keeps receiving Art-Net and retries the device connection once per second. Output resumes with the latest valid frame when the interface returns.

The ENTTEC Open is not a buffered professional interface. Flux configures the required DMX serial settings and generates the DMX BREAK and Mark After Break for every frame, but timing still depends on the host computer and USB scheduling.

- The interface does not provide an autonomous frame buffer comparable to an ENTTEC DMX USB Pro.
- It can be more sensitive to computer load than a buffered interface.
- Only one application can own the device at a time.

Close QLC+, FreeStyler, MagicQ, or any other application that has the device open before starting Flux. Test the complete chain with fixtures before using it in a show.

## Build and releases

Flux uses Rust stable and Edition 2024:

```shell
cargo build --release
```

`libftd2xx` statically links the vendor D2XX library, so release builds do not require a separately installed D2XX DLL or shared object. On Linux, make sure the current user has access to the USB device, normally through an appropriate udev rule.

The **Release** GitHub Actions workflow runs from `main`, bumps the selected semantic version, builds Windows and Linux Tauri bundles, and publishes checksums. Flux also publishes signed updater artifacts and `latest.json` for the Tauri updater. The updater public key is embedded in `tauri.conf.json`; do not replace it after publishing releases without a key-rotation migration strategy.

## Direction

Flux will remain a direct operational bridge before it becomes a broader platform. Near-term work focuses on stronger Art-Net behavior, diagnostics, and dependable desktop operation. A second DMX backend, such as ENTTEC DMX USB Pro, or a second input protocol, such as sACN / E1.31, will be considered only when there is a concrete operating need for it.

The goal for 1.0 is simple: reliable Art-Net and Open DMX operation within the hardware limits, clear failure recovery, tested Windows support, viable Linux support, reproducible releases, and documentation that operators can use during setup and shows.
