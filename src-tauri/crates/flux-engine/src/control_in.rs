//! The inputs of the remote control: a MIDI port and a UDP socket that turn what arrives into
//! [`Input`]s on a channel. What to do with them is the app's ([`crate::control`] decodes and maps).
//!
//! Nothing here logs: the MIDI callback runs on the driver's thread and the socket thread is part
//! of the real-time side, so both only decode and send. The thread that receives the inputs
//! reports.

use std::io;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use midir::{Ignore, MidiInput, MidiInputConnection};

use crate::control::{ControlAction, MidiTrigger, decode_midi, decode_osc, osc_action};
use crate::devices::find_by_name;

const MIDI_CLIENT_NAME: &str = "FLUX control";

/// How long the socket thread waits for a packet before it looks at the stop flag again.
const POLL: Duration = Duration::from_millis(100);

/// Room for any OSC packet a control surface sends; the rest of a bigger datagram is lost.
const PACKET: usize = 4096;

/// What the inputs deliver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A pad or a control was pressed. Which action it is depends on the user's bindings.
    Midi(MidiTrigger),
    /// An OSC message asked for an action, by one of the fixed addresses.
    Action(ControlAction),
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
                    if let Some(trigger) = decode_midi(bytes) {
                        // The receiver is gone while the listener is being replaced.
                        let _ = inputs.send(Input::Midi(trigger));
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
            if let Some(action) = osc_action(&message) {
                let _ = inputs.send(Input::Action(action));
            }
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

    #[test]
    fn a_packet_should_arrive_as_the_action_it_asks_for() {
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
            Ok(Input::Action(ControlAction::Play))
        );
        assert_eq!(
            rx.recv_timeout(WAIT),
            Ok(Input::Action(ControlAction::GotoCue(3)))
        );
    }

    #[test]
    fn what_is_not_a_request_should_be_dropped_and_the_next_packet_still_heard() {
        let (tx, rx) = mpsc::channel();
        let listener = OscListener::bind(0, tx).unwrap();

        send_to(listener.port(), b"not osc at all");
        send_to(listener.port(), &crate::osc::encode("/other/thing", &[]));
        send_to(listener.port(), &crate::osc::encode("/flux/stop", &[]));

        assert_eq!(
            rx.recv_timeout(WAIT),
            Ok(Input::Action(ControlAction::Stop))
        );
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
}
