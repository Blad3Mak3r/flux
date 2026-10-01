use std::net::SocketAddr;

use clap::{ArgAction, Parser};

#[derive(Clone, Debug, Parser)]
#[command(version, about = "A lightweight real-time lighting data bridge")]
pub struct Cli {
    /// Art-Net UDP address to listen on.
    #[arg(long, default_value = "127.0.0.1:6454")]
    pub listen: SocketAddr,

    /// Art-Net Port-Address to forward (0 to 32767).
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u16).range(0..=32767))]
    pub universe: u16,

    /// FTDI serial number. Required when multiple FTDI devices are connected.
    #[arg(long)]
    pub device: Option<String>,

    /// Number of DMX channels to transmit (1 to 512).
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u16).range(1..=512))]
    pub channels: u16,

    /// DMX output refresh rate in Hz (1 to 44).
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u16).range(1..=44))]
    pub fps: u16,

    /// List detected FTDI devices and exit.
    #[arg(long)]
    pub list_devices: bool,

    /// Receive and inspect Art-Net without opening an FTDI device.
    #[arg(long)]
    pub dry_run: bool,

    /// Run without Tauri, as a headless console session.
    #[arg(long)]
    pub headless: bool,

    /// Show diagnostic logging. Repeat for trace-level output.
    #[arg(short, long, action = ArgAction::Count)]
    pub verbose: u8,
}
