# Flux

> A lightweight real-time bridge for routing lighting data between protocols and interfaces.

Flux is a small Rust application for moving lighting data between a network protocol and a physical interface. Its first implementation receives Art-Net `ArtDmx` and continuously drives an ENTTEC Open DMX USB through FTDI D2XX.

It deliberately solves one route well instead of being a generic lighting framework. New sources or outputs can be added when there is a real second implementation to justify an abstraction.

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

## First Light (v0.1)

- Art-Net `ArtDmx` receiver, including loopback use.
- One Art-Net Port-Address selected with `--universe`.
- Last valid DMX frame buffering: transmission begins only after the first valid frame and then retains that look if Art-Net disappears.
- ENTTEC Open DMX USB output through FTDI D2XX at a stable, host-driven refresh rate.
- FTDI enumeration and safe selection by serial number.
- `--dry-run` for verifying Art-Net without hardware.
- Tests for ArtDmx parsing and DMX frame handling.
- Windows and Linux CI checks.

## Quick start: MagicQ loopback to ENTTEC Open

In MagicQ, open:

`SETUP` → `VIEW SETTINGS` → `Network`

Set **Net Host Options** to **Normal + Loopback IP**.

Then open:

`SETUP` → `VIEW DMX I/O`

Configure Universe 1 as:

| Setting | Value |
| --- | --- |
| Status | Enabled |
| Out Type | Art-Net |
| Out Uni | 0 |

The route is then:

```
MagicQ U1 -> Art-Net Universe 0 -> 127.0.0.1:6454 -> Flux -> ENTTEC Open
```

Connect the ENTTEC Open DMX USB, make sure no other program has it open, and run:

```shell
flux
```

The defaults are:

| Option | Default |
| --- | --- |
| Listen address | `127.0.0.1:6454` |
| Art-Net universe | `0` |
| DMX channels | `512` |
| Refresh rate | `30 Hz` |

For an Art-Net sender on the LAN, listen on all IPv4 interfaces:

```shell
flux --listen 0.0.0.0:6454
```

Any software that sends valid `ArtDmx` packets can use the same route:

```
Art-Net sender -> Flux -> ENTTEC Open DMX USB -> DMX512
```

## CLI

```text
flux
flux --listen 0.0.0.0:6454
flux --universe 0
flux --channels 256 --fps 30
flux --list-devices
flux --device FTXXXXXXXX
flux --dry-run -v
```

Use `flux --help` for the complete option reference and `flux --version` for the installed version.

`--list-devices` lists the serial numbers reported by D2XX. If more than one FTDI device is connected, Flux refuses to guess; select the required device with `--device <SERIAL>`.

`--dry-run` receives and validates Art-Net, filters the configured universe and updates the in-memory frame, but does not open an FTDI device or transmit physical DMX. Add `-v` to see frame-update diagnostics without logging every received packet.

## Building

Flux uses Rust stable and Edition 2024:

```shell
cargo build --release
```

Flux statically links the vendor D2XX library through `libftd2xx`, so the release binary does not depend on a separately installed D2XX DLL/shared object. On Linux, ensure the current user has permission to access the device (usually with a suitable udev rule).

## Timing and hardware limits

The ENTTEC Open is not a buffered DMX interface. Flux configures it for 250000 baud, 8 data bits, no parity and 2 stop bits, then generates a safe DMX BREAK and Mark After Break before every frame. It uses a short spin wait only for those sub-millisecond timing windows, and an absolute refresh schedule for the rest of the frame interval to avoid drift.

This does **not** turn an Open DMX USB into a professional interface with autonomous output:

- Timing depends on the computer and USB scheduling.
- The interface does not maintain a comparable autonomous frame buffer.
- It can be more sensitive to system load than an interface such as DMX USB Pro.
- Only one application can open it at a time.

Close QLC+, FreeStyler, MagicQ, or any other application that currently owns the device before starting Flux.

Flux v0.1 is experimental software. Test the complete chain with your fixtures before using it in a show.

## Roadmap

### v0.1 — First Light

ArtDmx receiver, loopback, universe selection, last-frame buffering, ENTTEC Open via D2XX, continuous DMX, device enumeration and selection, `--channels`, `--fps`, `--dry-run`, logging, tests, CI, MagicQ documentation, and a Windows release.

### v0.2 — Reliability

USB-disconnect detection, automatic ENTTEC reconnection, RX/TX statistics, Art-Net timeout warnings, improved Ctrl+C shutdown, diagnostics, and stress testing.

### v0.3 — Network

Optional source-IP filtering, better `ArtDmx` sequence handling, stronger multi-NIC behavior, LAN documentation, and an ArtSync evaluation.

### v0.4 — Second DMX backend

Evaluate ENTTEC DMX USB Pro. Only with two concrete outputs will Flux consider a shared output abstraction.

### v0.5 — Second network protocol

Add sACN / E1.31. Only with two concrete network inputs will Flux consider a shared source abstraction.

### v1.0

Stable CLI and behavior, reliable reconnection, dependable Art-Net and Open DMX operation within the hardware limits, tested Windows support, viable Linux support, reproducible releases, complete documentation, and no known severe signal-loss bugs.
