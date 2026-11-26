//! The inputs of the remote control: a MIDI port and a UDP socket that turn what arrives into
//! [`Input`]s on a channel. What to do with them is the app's ([`crate::control`] decodes and maps).
//!
//! Nothing here logs: the MIDI callback runs on the driver's thread and the socket thread is part
//! of the real-time side, so both only decode and send. The thread that receives the inputs
//! reports.

use std::io;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use midir::{Ignore, MidiInput, MidiInputConnection};

use crate::artnet::{ARTNET_PORT, DmxFrame, parse_art_dmx};
use crate::control::{KeyMsg, MidiMsg, OscMessage, decode_midi_message, decode_osc};
use crate::devices::find_by_name;

const MIDI_CLIENT_NAME: &str = "FLUX control";

/// How long the socket thread waits for a packet before it looks at the stop flag again.
const POLL: Duration = Duration::from_millis(100);

/// Room for any OSC packet a control surface sends; the rest of a bigger datagram is lost.
const PACKET: usize = 4096;

/// What the inputs deliver, decoded but not interpreted: what a message does depends on the
/// bindings of the cues and the mappings of the VJ grid, which the thread that receives it knows.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A note, control change, program change or pitch bend of the port that is open.
    Midi(MidiMsg),
    /// An OSC message, from the socket.
    Osc(OscMessage),
    /// The values of one Art-Net universe.
    Dmx(DmxFrame),
    /// A key of the keyboard, forwarded by the window.
    Key(KeyMsg),
}

#[derive(Debug, thiserror::Error)]
pub enum ListenError {
    #[error("MIDI is unavailable: {0}")]
    Unavailable(String),
    /// The port the user chose is not connected. A normal state, not a failure.
    #[error("the MIDI port is not connected")]
    Missing,
}

/// A MIDI input port that is open. Dropping it closes the port.
pub struct MidiListener {
    name: String,
    _connection: MidiInputConnection<()>,
}

impl MidiListener {
    /// Opens the port a stored name hint refers to (`index` picks among ports of the same name)
    /// and sends the triggers it receives to `inputs`.
    pub fn open(hint: &str, index: usize, inputs: Sender<Input>) -> Result<Self, ListenError> {
        let mut client = MidiInput::new(MIDI_CLIENT_NAME)
            .map_err(|err| ListenError::Unavailable(err.to_string()))?;
        client.ignore(Ignore::All);
        let ports: Vec<_> = client
            .ports()
            .into_iter()
            .filter_map(|port| Some((client.port_name(&port).ok()?, port)))
            .collect();
        let (name, port) = find_by_name(hint, &ports, index, |(name, _)| name)
            .cloned()
            .ok_or(ListenError::Missing)?;
        let connection = client
            .connect(
                &port,
                "Flux control in",
                move |_, bytes, ()| {
                    if let Some(message) = decode_midi_message(bytes) {
                        // The receiver is gone while the listener is being replaced.
                        let _ = inputs.send(Input::Midi(message));
                    }
                },
                (),
            )
            .map_err(|err| ListenError::Unavailable(err.to_string()))?;
        Ok(Self {
            name,
            _connection: connection,
        })
    }

    /// The name of the port as the system lists it.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A UDP socket that listens for OSC. Dropping it stops the thread and frees the port.
pub struct OscListener {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl OscListener {
    /// Listens on `port` of every interface (`0` picks a free one, which [`Self::port`] tells) and
    /// sends the actions the packets ask for to `inputs`. Fails when the port is taken.
    pub fn bind(port: u16, inputs: Sender<Input>) -> io::Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", port))?;
        socket.set_read_timeout(Some(POLL))?;
        let port = socket.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new().name("flux-osc-in".into()).spawn({
            let stop = Arc::clone(&stop);
            move || run(&socket, &inputs, &stop)
        })?;
        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

fn run(socket: &UdpSocket, inputs: &Sender<Input>, stop: &AtomicBool) {
    let mut packet = [0_u8; PACKET];
    while !stop.load(Ordering::Relaxed) {
        // A timeout or a failed read: look at the stop flag and try again.
        let Ok(length) = socket.recv(&mut packet) else {
            continue;
        };
        for message in decode_osc(&packet[..length]) {
            let _ = inputs.send(Input::Osc(message));
        }
    }
}

impl Drop for OscListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A UDP socket that listens for Art-Net (ArtDmx). Dropping it stops the thread and frees the
/// port.
pub struct ArtnetListener {
    port: u16,
    packets: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ArtnetListener {
    /// Listens on `address` (a bind interface; `0.0.0.0` is every one) at `port` ([`ARTNET_PORT`]
    /// in the app, `0` picks a free one) and sends the frames of `universe` (every one when
    /// `None`) to `inputs`. The port is shared (`SO_REUSEADDR`, and `SO_REUSEPORT` on Unix), as lighting software does, so
    /// another program that listens does not keep Flux from it.
    pub fn bind(
        address: &str,
        port: u16,
        universe: Option<u16>,
        inputs: Sender<Input>,
    ) -> io::Result<Self> {
        let address: std::net::IpAddr = address
            .parse()
            .map_err(|_| io::Error::other("invalid Art-Net bind address"))?;
        let socket = socket2::Socket::new(
            socket2::Domain::for_address((address, port).into()),
            socket2::Type::DGRAM,
            Some(socket2::Protocol::UDP),
        )?;
        socket.set_reuse_address(true)?;
        // macOS and the BSDs share a UDP port only with `SO_REUSEPORT` as well.
        #[cfg(unix)]
        socket.set_reuse_port(true)?;
        socket.bind(&std::net::SocketAddr::new(address, port).into())?;
        let socket: UdpSocket = socket.into();
        socket.set_read_timeout(Some(POLL))?;
        let port = socket.local_addr()?.port();
        let packets = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new()
            .name("flux-artnet-in".into())
            .spawn({
                let (stop, packets) = (Arc::clone(&stop), Arc::clone(&packets));
                move || run_artnet(&socket, &inputs, universe, &packets, &stop)
            })?;
        Ok(Self {
            port,
            packets,
            stop,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// How many ArtDmx packets of the wanted universes arrived since it was opened.
    pub fn packets(&self) -> u64 {
        self.packets.load(Ordering::Relaxed)
    }

    /// The counter, to read after the listener moved.
    pub fn counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.packets)
    }
}

fn run_artnet(
    socket: &UdpSocket,
    inputs: &Sender<Input>,
    universe: Option<u16>,
    packets: &AtomicU64,
    stop: &AtomicBool,
) {
    let mut packet = [0_u8; PACKET];
    while !stop.load(Ordering::Relaxed) {
        let Ok(length) = socket.recv(&mut packet) else {
            continue;
        };
        let Some(frame) = parse_art_dmx(&packet[..length]) else {
            continue;
        };
        if universe.is_some_and(|wanted| wanted != frame.universe) {
            continue;
        }
        packets.fetch_add(1, Ordering::Relaxed);
        let _ = inputs.send(Input::Dmx(frame));
    }
}

impl Drop for ArtnetListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The port of Art-Net, for the page to show.
pub const ARTNET_UDP_PORT: u16 = ARTNET_PORT;

#[cfg(test)]
mod tests {
    use super::*;
    use flux_project::OscArg;
    use std::sync::mpsc;

    const WAIT: Duration = Duration::from_secs(2);

    fn send_to(port: u16, packet: &[u8]) {
        UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .send_to(packet, ("127.0.0.1", port))
            .unwrap();
    }

    fn osc(address: &str, args: &[OscArg]) -> Input {
        Input::Osc(OscMessage {
            address: address.into(),
            args: args.to_vec(),
        })
    }

    #[test]
    fn a_packet_should_arrive_as_the_message_it_holds() {
        let (tx, rx) = mpsc::channel();
        let listener = OscListener::bind(0, tx).unwrap();

        send_to(
            listener.port(),
            &crate::osc::encode("/flux/play", &[OscArg::Int(1)]),
        );
        send_to(
            listener.port(),
            &crate::osc::encode("/flux/cue/goto", &[OscArg::Int(3)]),
        );

        assert_eq!(
            rx.recv_timeout(WAIT),
            Ok(osc("/flux/play", &[OscArg::Int(1)]))
        );
        assert_eq!(
            rx.recv_timeout(WAIT),
            Ok(osc("/flux/cue/goto", &[OscArg::Int(3)]))
        );
    }

    #[test]
    fn what_is_not_osc_should_be_dropped_and_the_next_packet_still_heard() {
        let (tx, rx) = mpsc::channel();
        let listener = OscListener::bind(0, tx).unwrap();

        send_to(listener.port(), b"not osc at all");
        send_to(listener.port(), &crate::osc::encode("/flux/stop", &[]));

        assert_eq!(rx.recv_timeout(WAIT), Ok(osc("/flux/stop", &[])));
    }

    #[test]
    fn a_port_in_use_should_not_bind_and_a_dropped_listener_should_free_it() {
        let (tx, _rx) = mpsc::channel();
        let first = OscListener::bind(0, tx.clone()).unwrap();
        let port = first.port();

        assert!(OscListener::bind(port, tx.clone()).is_err());
        drop(first);
        assert!(OscListener::bind(port, tx).is_ok());
    }

    fn art_packet(universe: u8, data: &[u8]) -> Vec<u8> {
        let mut bytes = b"Art-Net\0".to_vec();
        bytes.extend([0x00, 0x50, 0, 14, 0, 0, universe, 0]);
        bytes.extend((data.len() as u16).to_be_bytes());
        bytes.extend(data);
        bytes
    }

    #[test]
    fn art_net_should_arrive_as_frames_be_filtered_by_universe_and_counted() {
        let (tx, rx) = mpsc::channel();
        let listener = ArtnetListener::bind("127.0.0.1", 0, Some(2), tx).unwrap();

        send_to(listener.port(), b"not art-net");
        send_to(listener.port(), &art_packet(1, &[1, 2]));
        send_to(listener.port(), &art_packet(2, &[3, 4]));

        let Ok(Input::Dmx(frame)) = rx.recv_timeout(WAIT) else {
            panic!("the frame of universe 2 should arrive");
        };
        assert_eq!((frame.universe, frame.data), (2, vec![3, 4]));
        assert_eq!(listener.packets(), 1, "only the wanted universe is counted");
    }

    #[test]
    fn the_art_net_port_should_be_shared_and_freed_when_dropped() {
        let (tx, _rx) = mpsc::channel();
        let first = ArtnetListener::bind("127.0.0.1", 0, None, tx.clone()).unwrap();
        let port = first.port();

        assert!(ArtnetListener::bind("127.0.0.1", port, None, tx.clone()).is_ok());
        assert!(ArtnetListener::bind("not an address", 0, None, tx).is_err());
    }
}
