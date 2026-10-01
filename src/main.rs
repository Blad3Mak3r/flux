#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod artnet;
mod cli;
mod dmx;
mod enttec;
mod runtime_control;
mod settings;
mod state;
mod ui;
#[cfg(windows)]
mod windows_console;

use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use tracing::{Level, debug, info};

use crate::cli::Cli;
use crate::dmx::{DmxFrame, validate_channel_count};
use crate::state::RuntimeStatus;

pub type LatestFrame = Arc<Mutex<Option<DmxFrame>>>;
pub type SharedStatus = Arc<Mutex<RuntimeStatus>>;
pub type Shutdown = Arc<AtomicBool>;

fn main() -> Result<()> {
    #[cfg(windows)]
    windows_console::attach_parent();

    let mut cli = Cli::parse();
    init_logging(cli.verbose);

    if cli.list_devices {
        return enttec::print_devices();
    }

    if should_load_saved_settings()
        && let Some(saved) = settings::load(&settings::default_directory())?
    {
        cli.listen = saved.listen;
        cli.universe = saved.universe;
        cli.device = saved.device;
        cli.channels = saved.channels;
        cli.fps = saved.fps;
        info!("Loaded last Flux configuration");
    }

    let channels = validate_channel_count(cli.channels).expect("clap validates channels");
    print_startup(&cli, channels);

    let status = Arc::new(Mutex::new(RuntimeStatus::new(&cli, channels)));
    let latest = Arc::new(Mutex::new(None));
    let shutdown = Arc::new(AtomicBool::new(false));
    if cli.headless {
        return run_runtime(cli, latest, status, shutdown);
    }

    let saved_settings = settings::SavedSettings::from(&cli);
    let runtime = Arc::new(runtime_control::RuntimeControl::new(saved_settings.clone()));
    let worker_cli = cli;
    let worker_latest = Arc::clone(&latest);
    let worker_status = Arc::clone(&status);
    let worker_shutdown = Arc::clone(&shutdown);
    let worker_runtime = Arc::clone(&runtime);
    let supervisor = thread::spawn(move || {
        run_supervisor(
            worker_cli,
            worker_latest,
            worker_status,
            worker_shutdown,
            worker_runtime,
        )
    });

    let ui_result = ui::run(ui::UiState {
        latest,
        status,
        shutdown: Arc::clone(&shutdown),
        settings: Mutex::new(saved_settings),
        runtime,
    });
    shutdown.store(true, Ordering::Relaxed);
    if supervisor.join().is_err() {
        tracing::error!("Flux runtime supervisor panicked during shutdown");
    }
    ui_result
}

fn run_runtime(
    cli: Cli,
    latest: LatestFrame,
    status: SharedStatus,
    shutdown: Shutdown,
) -> Result<()> {
    let channels =
        validate_channel_count(cli.channels).expect("settings are validated before reload");
    let output = if cli.dry_run {
        info!("Dry-run enabled; no FTDI device will be opened");
        status
            .lock()
            .expect("runtime status mutex poisoned")
            .set_output("Dry-run (no physical output)", None);
        None
    } else {
        status
            .lock()
            .expect("runtime status mutex poisoned")
            .set_output("Looking for FTDI device", None);
        let output_latest = Arc::clone(&latest);
        let output_status = Arc::clone(&status);
        let output_shutdown = Arc::clone(&shutdown);
        let serial = cli.device.clone();
        Some(thread::spawn(move || {
            enttec::run_reconnecting(
                serial,
                channels,
                cli.fps,
                output_latest,
                output_status,
                output_shutdown,
            )
        }))
    };

    let receiver_result = receive_artnet(
        cli.listen,
        cli.universe,
        latest,
        cli.dry_run,
        status,
        Arc::clone(&shutdown),
    );
    shutdown.store(true, Ordering::Relaxed);
    if let Some(output) = output {
        output
            .join()
            .map_err(|_| anyhow!("DMX output thread panicked"))??;
    }
    receiver_result
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

fn should_load_saved_settings() -> bool {
    const CONFIG_OPTIONS: [&str; 5] = ["--listen", "--universe", "--device", "--channels", "--fps"];
    !std::env::args().skip(1).any(|argument| {
        CONFIG_OPTIONS
            .iter()
            .any(|option| argument == *option || argument.starts_with(&format!("{option}=")))
    })
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
    status
        .lock()
        .expect("runtime status mutex poisoned")
        .set_artnet_listening();
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
                    status
                        .lock()
                        .expect("runtime status mutex poisoned")
                        .set_artnet_offline();
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
                status
                    .lock()
                    .expect("runtime status mutex poisoned")
                    .record_artnet_packet(source, packet.sequence);
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

fn run_supervisor(
    base_cli: Cli,
    latest: LatestFrame,
    status: SharedStatus,
    shutdown: Shutdown,
    runtime: runtime_control::SharedRuntimeControl,
) {
    let mut generation = runtime.generation();
    while !shutdown.load(Ordering::Relaxed) {
        let settings = runtime.snapshot();
        let mut cli = base_cli.clone();
        cli.listen = settings.listen;
        cli.universe = settings.universe;
        cli.device = settings.device.clone();
        cli.channels = settings.channels;
        cli.fps = settings.fps;

        status
            .lock()
            .expect("runtime status mutex poisoned")
            .apply_settings(&settings);
        *latest.lock().expect("latest frame mutex poisoned") = None;
        let session_shutdown = Arc::new(AtomicBool::new(false));
        let session_latest = Arc::clone(&latest);
        let session_status = Arc::clone(&status);
        let session_stop = Arc::clone(&session_shutdown);
        let worker =
            thread::spawn(move || run_runtime(cli, session_latest, session_status, session_stop));

        while !shutdown.load(Ordering::Relaxed) && runtime.generation() == generation {
            thread::sleep(Duration::from_millis(100));
        }
        session_shutdown.store(true, Ordering::Relaxed);
        let session_failed = match worker.join() {
            Ok(Ok(())) => false,
            Ok(Err(error)) => {
                tracing::error!(%error, "Flux runtime session stopped");
                true
            }
            Err(_) => {
                tracing::error!("Flux runtime session panicked");
                true
            }
        };
        generation = runtime.generation();
        if session_failed && !shutdown.load(Ordering::Relaxed) {
            wait_for_runtime_retry(&shutdown, &runtime, generation);
        }
    }
}

fn wait_for_runtime_retry(
    shutdown: &AtomicBool,
    runtime: &runtime_control::SharedRuntimeControl,
    generation: u64,
) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while !shutdown.load(Ordering::Relaxed)
        && runtime.generation() == generation
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(100));
    }
}
