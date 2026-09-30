use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use libftd2xx::{BitsPerWord, DeviceInfo, Ftdi, FtdiCommon, Parity, StopBits, list_devices};

use crate::dmx::DmxFrame;

const DMX_BAUD_RATE: u32 = 250_000;
const BREAK_DURATION: Duration = Duration::from_micros(176);
const MAB_DURATION: Duration = Duration::from_micros(16);

pub fn devices() -> Result<Vec<DeviceInfo>> {
    list_devices().context("Unable to enumerate FTDI devices. Is the FTDI D2XX driver installed?")
}

pub fn print_devices() -> Result<()> {
    let devices = devices()?;
    if devices.is_empty() {
        println!("No FTDI devices found.");
        return Ok(());
    }

    for device in devices {
        println!(
            "{}  {}  {:?} (VID:PID {:04X}:{:04X}){}",
            device.serial_number,
            device.description,
            device.device_type,
            device.vendor_id,
            device.product_id,
            if device.port_open { " [in use]" } else { "" },
        );
    }
    Ok(())
}

pub fn select_device(serial: Option<&str>) -> Result<DeviceInfo> {
    let devices = devices()?;
    if devices.is_empty() {
        bail!(
            "No FTDI devices found. Connect an ENTTEC Open DMX USB and install the FTDI D2XX driver."
        );
    }

    if let Some(serial) = serial {
        return devices
            .into_iter()
            .find(|device| device.serial_number == serial)
            .with_context(|| format!("No FTDI device with serial number {serial:?} was found"));
    }

    if devices.len() == 1 {
        return Ok(devices.into_iter().next().expect("length checked"));
    }

    let choices = devices
        .iter()
        .map(|device| format!("  {}  {}", device.serial_number, device.description))
        .collect::<Vec<_>>()
        .join("\n");
    bail!(
        "More than one FTDI device was found. Select the ENTTEC Open DMX USB with --device <SERIAL>:\n{choices}"
    );
}

pub fn run(
    device: DeviceInfo,
    channels: usize,
    fps: u16,
    latest: crate::LatestFrame,
) -> Result<()> {
    let mut ftdi = Ftdi::with_serial_number(&device.serial_number)
        .map_err(|error| anyhow::anyhow!(error))
        .with_context(|| {
            format!(
                "Unable to open FTDI device {}. It may already be in use by another application such as QLC+",
                device.serial_number
            )
        })?;

    configure(&mut ftdi)?;
    tracing::info!(device = %device.serial_number, "ENTTEC Open DMX USB ready");

    let interval = Duration::from_secs_f64(1.0 / f64::from(fps));
    let mut next_frame = Instant::now();
    let mut output_started = false;

    loop {
        let frame = latest.lock().expect("latest frame mutex poisoned").clone();
        let Some(frame) = frame else {
            thread::sleep(Duration::from_millis(5));
            next_frame = Instant::now();
            continue;
        };

        if !output_started {
            tracing::info!("DMX output started");
            output_started = true;
        }

        send_frame(&mut ftdi, &frame, channels)?;
        next_frame += interval;
        wait_until(next_frame);
        while next_frame <= Instant::now() {
            next_frame += interval;
        }
    }
}

fn configure(ftdi: &mut Ftdi) -> Result<()> {
    ftdi.reset().context("Unable to reset FTDI device")?;
    ftdi.set_baud_rate(DMX_BAUD_RATE)
        .context("Unable to set FTDI baud rate to 250000")?;
    ftdi.set_data_characteristics(BitsPerWord::Bits8, StopBits::Bits2, Parity::No)
        .context("Unable to configure FTDI for 8N2")?;
    ftdi.set_flow_control_none()
        .context("Unable to disable FTDI flow control")?;
    ftdi.set_timeouts(Duration::from_millis(100), Duration::from_millis(100))
        .context("Unable to configure FTDI timeouts")?;
    Ok(())
}

fn send_frame(ftdi: &mut Ftdi, frame: &DmxFrame, channels: usize) -> Result<()> {
    ftdi.set_break_on().context("Unable to start DMX BREAK")?;
    busy_wait(BREAK_DURATION);
    ftdi.set_break_off().context("Unable to end DMX BREAK")?;
    busy_wait(MAB_DURATION);

    let mut data = [0_u8; crate::dmx::DMX_CHANNELS + 1];
    data[1..].copy_from_slice(frame.slots());
    ftdi.write_all(&data[..=channels])
        .context("Unable to transmit DMX data")?;
    Ok(())
}

fn busy_wait(duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

fn wait_until(deadline: Instant) {
    const SPIN_THRESHOLD: Duration = Duration::from_micros(250);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let remaining = deadline - now;
        if remaining > SPIN_THRESHOLD {
            thread::sleep(remaining - SPIN_THRESHOLD);
        } else {
            std::hint::spin_loop();
        }
    }
}
