mod artnet;
mod cli;
mod dmx;
mod enttec;
mod tray;

use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use tracing::{Level, debug, info};

use crate::cli::Cli;
use crate::dmx::{DmxFrame, validate_channel_count};

type LatestFrame = Arc<Mutex<Option<DmxFrame>>>;

#[derive(Debug)]
pub struct RuntimeStatus {
    artnet: String,
    dmx: String,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self {
            artnet: "waiting for ArtDmx".to_owned(),
            dmx: "waiting for Art-Net".to_owned(),
        }
    }
}

type SharedStatus = Arc<Mutex<RuntimeStatus>>;
type Shutdown = Arc<AtomicBool>;

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    if cli.list_devices {
        return enttec::print_devices();
    }

    let channels = validate_channel_count(cli.channels).expect("clap validates channels");
    print_startup(&cli, channels);

    let status = Arc::new(Mutex::new(RuntimeStatus::default()));
    let shutdown = Arc::new(AtomicBool::new(false));
    if cli.no_tray {
        return run_runtime(cli, channels, status, shutdown);
    }

    let worker_status = Arc::clone(&status);
    let worker_shutdown = Arc::clone(&shutdown);
    let worker = thread::spawn(move || {
        let result = run_runtime(
            cli,
            channels,
            Arc::clone(&worker_status),
            Arc::clone(&worker_shutdown),
        );
        if let Err(error) = &result {
            worker_status
                .lock()
                .expect("runtime status mutex poisoned")
                .dmx = format!("error: {error}");
            tracing::error!(%error, "Flux runtime stopped");
            worker_shutdown.store(true, Ordering::Relaxed);
        }
        result
    });

    let tray_result = tray::run(status, Arc::clone(&shutdown));
    shutdown.store(true, Ordering::Relaxed);
    let runtime_result = worker
        .join()
        .map_err(|_| anyhow!("Flux runtime thread panicked"))?;
    tray_result?;
    runtime_result
}

fn run_runtime(cli: Cli, channels: usize, status: SharedStatus, shutdown: Shutdown) -> Result<()> {
    let latest = Arc::new(Mutex::new(None));
    if cli.dry_run {
        info!("Dry-run enabled; no FTDI device will be opened");
        status.lock().expect("runtime status mutex poisoned").dmx =
            "dry-run (no physical output)".to_owned();
    } else {
        status.lock().expect("runtime status mutex poisoned").dmx =
            "selecting FTDI device".to_owned();
        let device = enttec::select_device(cli.device.as_deref())?;
        info!(
            device = %device.serial_number,
            description = %device.description,
            "Selected FTDI device"
        );
        let output_latest = Arc::clone(&latest);
        let output_status = Arc::clone(&status);
        let output_shutdown = Arc::clone(&shutdown);
        thread::spawn(move || {
            if let Err(error) = enttec::run(
                device,
                channels,
                cli.fps,
                output_latest,
                output_status,
                output_shutdown,
            ) {
                tracing::error!(%error, "DMX output stopped");
            }
        });
    }

    receive_artnet(
        cli.listen,
        cli.universe,
        latest,
        cli.dry_run,
        status,
        shutdown,
    )
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
    status: SharedStatus,
    shutdown: Shutdown,
) -> Result<()> {
    let socket = UdpSocket::bind(listen)
        .with_context(|| format!("Unable to listen for Art-Net on {listen}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(100)))
        .context("Unable to configure Art-Net receive timeout")?;
    let mut buffer = [0_u8; 600];
    let mut online = false;
    let mut last_packet_at: Option<Instant> = None;

    while !shutdown.load(Ordering::Relaxed) {
        let (size, source) = match socket.recv_from(&mut buffer) {
            Ok(packet) => packet,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                if online
                    && last_packet_at.is_some_and(|last| last.elapsed() > Duration::from_secs(1))
                {
                    online = false;
                    status.lock().expect("runtime status mutex poisoned").artnet =
                        "offline — keeping last frame".to_owned();
                }
                continue;
            }
            Err(error) => return Err(error).context("Art-Net receive failed"),
        };
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
                last_packet_at = Some(Instant::now());
                status.lock().expect("runtime status mutex poisoned").artnet =
                    format!("universe {universe} online from {source}");
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

    Ok(())
}
