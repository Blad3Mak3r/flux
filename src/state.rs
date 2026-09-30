use std::net::SocketAddr;
use std::time::Instant;

use serde::Serialize;

use crate::cli::Cli;
use crate::LatestFrame;

#[derive(Debug)]
pub struct RuntimeStatus {
    pub artnet: String,
    pub dmx: String,
    listen: String,
    universe: u16,
    channels: usize,
    fps: u16,
    artnet_state: &'static str,
    source: Option<String>,
    sequence: Option<u8>,
    last_packet: Option<Instant>,
    packets: u64,
    rate_started: Instant,
    output_state: String,
    device: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeSnapshot {
    pub artnet_state: String,
    pub listen: String,
    pub universe: u16,
    pub packets_per_second: u64,
    pub last_packet_ms: Option<u128>,
    pub source: Option<String>,
    pub sequence: Option<u8>,
    pub output_state: String,
    pub device: Option<String>,
    pub refresh_hz: u16,
    pub channels: usize,
    pub dmx: Vec<u8>,
}

impl RuntimeStatus {
    pub fn new(cli: &Cli, channels: usize) -> Self {
        Self {
            artnet: "waiting for ArtDmx".to_owned(),
            dmx: "waiting for Art-Net".to_owned(),
            listen: cli.listen.to_string(),
            universe: cli.universe,
            channels,
            fps: cli.fps,
            artnet_state: "Starting",
            source: None,
            sequence: None,
            last_packet: None,
            packets: 0,
            rate_started: Instant::now(),
            output_state: "Waiting for Art-Net".to_owned(),
            device: cli.device.clone(),
        }
    }

    pub fn set_artnet_listening(&mut self) {
        self.artnet_state = "Listening";
        self.artnet = "listening".to_owned();
    }

    pub fn set_artnet_offline(&mut self) {
        self.artnet_state = "Offline — keeping last frame";
        self.artnet = "offline — keeping last frame".to_owned();
    }

    pub fn record_artnet_packet(&mut self, source: SocketAddr, sequence: u8) {
        self.artnet_state = "Receiving";
        self.artnet = format!("receiving from {source}");
        self.source = Some(source.ip().to_string());
        self.sequence = (sequence != 0).then_some(sequence);
        self.last_packet = Some(Instant::now());
        self.packets += 1;
    }

    pub fn set_output(&mut self, state: impl Into<String>, device: Option<String>) {
        self.output_state = state.into();
        self.dmx = self.output_state.clone();
        if device.is_some() {
            self.device = device;
        }
    }

    pub fn snapshot(&mut self, latest: &LatestFrame) -> RuntimeSnapshot {
        let elapsed = self.rate_started.elapsed();
        let packets_per_second = (self.packets as f64 / elapsed.as_secs_f64()).round() as u64;
        if elapsed.as_secs() >= 1 {
            self.packets = 0;
            self.rate_started = Instant::now();
        }
        let dmx = latest
            .lock()
            .expect("latest frame mutex poisoned")
            .as_ref()
            .map(|frame| frame.slots().to_vec())
            .unwrap_or_else(|| vec![0; crate::dmx::DMX_CHANNELS]);
        RuntimeSnapshot {
            artnet_state: self.artnet.clone(),
            listen: self.listen.clone(),
            universe: self.universe,
            packets_per_second,
            last_packet_ms: self.last_packet.map(|packet| packet.elapsed().as_millis()),
            source: self.source.clone(),
            sequence: self.sequence,
            output_state: self.dmx.clone(),
            device: self.device.clone(),
            refresh_hz: self.fps,
            channels: self.channels,
            dmx,
        }
    }
}
