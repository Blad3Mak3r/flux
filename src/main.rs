mod artnet;
mod cli;
mod dmx;
mod enttec;

use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{Level, debug, info};

use crate::cli::Cli;
use crate::dmx::{DmxFrame, validate_channel_count};

type LatestFrame = Arc<Mutex<Option<DmxFrame>>>;

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    if cli.list_devices {
        return enttec::print_devices();
    }

    let channels = validate_channel_count(cli.channels).expect("clap validates channels");
    print_startup(&cli, channels);

    let latest = Arc::new(Mutex::new(None));
    if cli.dry_run {
        info!("Dry-run enabled; no FTDI device will be opened");
    } else {
        let device = enttec::select_device(cli.device.as_deref())?;
        info!(
            device = %device.serial_number,
            description = %device.description,
            "Selected FTDI device"
        );
        let output_latest = Arc::clone(&latest);
        thread::spawn(move || {
            if let Err(error) = enttec::run(device, channels, cli.fps, output_latest) {
                tracing::error!(%error, "DMX output stopped");
            }
        });
    }

    receive_artnet(cli.listen, cli.universe, latest, cli.dry_run)
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .without_time()
        .with_target(false)
        .init();
}

fn print_startup(cli: &Cli, channels: usize) {
    println!("Flux {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Art-Net");
    println!("  listen       {}", cli.listen);
    println!("  universe     {}", cli.universe);
    println!();
    println!("DMX");
    println!("  channels     {channels}");
    println!("  refresh      {} Hz", cli.fps);
    println!("  device       {}", cli.device.as_deref().unwrap_or("auto"));
    println!();
    println!("Waiting for Art-Net...");
}

fn receive_artnet(
    listen: std::net::SocketAddr,
    universe: u16,
    latest: LatestFrame,
    dry_run: bool,
) -> Result<()> {
    let socket = UdpSocket::bind(listen)
        .with_context(|| format!("Unable to listen for Art-Net on {listen}"))?;
    let mut buffer = [0_u8; 600];
    let mut online = false;

    loop {
        let (size, source) = socket
            .recv_from(&mut buffer)
            .context("Art-Net receive failed")?;
        match artnet::parse_art_dmx(&buffer[..size]) {
            Ok(packet) if packet.port_address == universe => {
                let frame = DmxFrame::from_channels(packet.data);
                let changed = {
                    let mut guard = latest.lock().expect("latest frame mutex poisoned");
                    let changed = guard.as_ref() != Some(&frame);
                    *guard = Some(frame);
                    changed
                };

                if !online {
                    online = true;
                    info!(universe, source = %source, "Art-Net universe online");
                }
                if dry_run && changed {
                    debug!(universe, channels = packet.data.len(), source = %source, "ArtDmx frame updated");
                }
            }
            Ok(packet) => debug!(
                received_universe = packet.port_address,
                requested_universe = universe,
                "Ignoring ArtDmx for another universe"
            ),
            Err(error) => debug!(?error, source = %source, "Ignoring invalid Art-Net packet"),
        }
    }
}
