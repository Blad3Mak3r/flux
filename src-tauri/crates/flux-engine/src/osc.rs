//! The OSC cue tracks: the packet of every message, the schedule a timeline makes of them and the
//! UDP sink that sends them.
//!
//! The encoder is the OSC 1.0 message format and nothing else (no bundles, no TCP), small enough
//! not to need a crate.

use std::net::{ToSocketAddrs, UdpSocket};

use flux_core::Clock;
use flux_project::{OscArg, OscDestination, OscEvent, Timeline, TrackKind};

use super::fire::{Planned, Schedule, Sink};

/// Appends `text` as an OSC string: its bytes, a NUL, and NULs up to a multiple of four.
fn push_string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(text.as_bytes());
    out.push(0);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// The OSC message for `address` and `args`.
pub fn encode(address: &str, args: &[OscArg]) -> Vec<u8> {
    let mut out = Vec::with_capacity(address.len() + 8 + args.len() * 4);
    push_string(&mut out, address);
    let mut tags = String::with_capacity(args.len() + 1);
    tags.push(',');
    for arg in args {
        tags.push(match arg {
            OscArg::Int(_) => 'i',
            OscArg::Float(_) => 'f',
            OscArg::String(_) => 's',
            OscArg::Bool(true) => 'T',
            OscArg::Bool(false) => 'F',
        });
    }
    push_string(&mut out, &tags);
    for arg in args {
        match arg {
            OscArg::Int(value) => out.extend_from_slice(&value.to_be_bytes()),
            OscArg::Float(value) => out.extend_from_slice(&value.to_be_bytes()),
            OscArg::String(text) => push_string(&mut out, text),
            OscArg::Bool(_) => {}
        }
    }
    out
}

fn encode_event(event: &OscEvent) -> Vec<u8> {
    encode(&event.address, &event.args)
}

/// What one destination is sent.
#[derive(Debug, Clone, PartialEq)]
pub struct OscPlan {
    pub destination: OscDestination,
    pub schedule: Schedule<Vec<u8>>,
}

/// The schedule of every destination the unmuted OSC tracks of `timeline` send to. Tracks on the
/// same destination share one schedule; a track whose destination is not in `destinations` (or has
/// none) sends nothing.
pub fn plan(timeline: &Timeline, destinations: &[OscDestination], clock: &Clock) -> Vec<OscPlan> {
    let mut plans: Vec<(&OscDestination, Vec<Planned<Vec<u8>>>)> = Vec::new();
    for track in timeline.tracks.iter().filter(|track| !track.muted) {
        let TrackKind::Osc {
            destination: Some(id),
            events,
        } = &track.kind
        else {
            continue;
        };
        let Some(destination) = destinations.iter().find(|d| &d.id == id) else {
            continue;
        };
        let at = match plans
            .iter()
            .position(|(other, _)| other.id == destination.id)
        {
            Some(at) => at,
            None => {
                plans.push((destination, Vec::new()));
                plans.len() - 1
            }
        };
        plans[at].1.extend(events.iter().map(|event| Planned {
            at: clock.frames_to_samples(event.at),
            payload: encode_event(event),
        }));
    }
    plans
        .into_iter()
        .map(|(destination, items)| OscPlan {
            destination: destination.clone(),
            schedule: Schedule::new(items),
        })
        .collect()
}

/// A UDP socket connected to a destination.
pub struct OscSink(UdpSocket);

impl OscSink {
    /// Resolves the destination's host and opens a socket to it. It can wait on a name lookup, so
    /// it is for the control thread, never for the one that fires. UDP needs no receiver to be
    /// there, so this only fails for a host that cannot be resolved or a socket that cannot open.
    pub fn connect(destination: &OscDestination) -> Result<Self, String> {
        let address = (destination.host.as_str(), destination.port)
            .to_socket_addrs()
            .map_err(|err| format!("{}: {err}", destination.host))?
            .next()
            .ok_or_else(|| format!("{}: no address", destination.host))?;
        let local = if address.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(local)
            .and_then(|socket| socket.connect(address).map(|()| socket))
            .map_err(|err| format!("{}: {err}", destination.host))?;
        // A full send buffer must drop the packet, not hold the firing thread.
        socket
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;
        Ok(Self(socket))
    }
}

impl Sink<Vec<u8>> for OscSink {
    fn send(&mut self, payload: &Vec<u8>) -> bool {
        self.0.send(payload).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_core::FrameRate;
    use flux_project::Track;

    #[test]
    fn a_message_without_arguments_should_be_the_padded_address_and_an_empty_tag_list() {
        assert_eq!(encode("/go", &[]), [b'/', b'g', b'o', 0, b',', 0, 0, 0]);
    }

    #[test]
    fn an_address_that_fills_a_word_should_still_end_with_a_nul_word() {
        // Four bytes of text need a whole extra word for the terminator.
        let bytes = encode("/abc", &[]);
        assert_eq!(&bytes[..8], [b'/', b'a', b'b', b'c', 0, 0, 0, 0]);
    }

    #[test]
    fn int_and_float_should_be_big_endian_words() {
        let bytes = encode("/a", &[OscArg::Int(1), OscArg::Float(0.5)]);
        assert_eq!(
            bytes,
            [
                b'/', b'a', 0, 0, // address
                b',', b'i', b'f', 0, // tags
                0, 0, 0, 1, // int 1
                0x3F, 0x00, 0x00, 0x00, // float 0.5
            ]
        );
    }

    #[test]
    fn a_string_should_be_padded_and_a_bool_should_have_no_data() {
        let bytes = encode(
            "/s",
            &[
                OscArg::String("hi".into()),
                OscArg::Bool(true),
                OscArg::Bool(false),
            ],
        );
        assert_eq!(
            bytes,
            [
                b'/', b's', 0, 0, //
                b',', b's', b'T', b'F', 0, 0, 0, 0, //
                b'h', b'i', 0, 0,
            ]
        );
    }

    #[test]
    fn a_packet_should_always_be_a_multiple_of_four_bytes() {
        for text in ["", "a", "ab", "abc", "abcd"] {
            let bytes = encode("/x", &[OscArg::String(text.into())]);
            assert_eq!(bytes.len() % 4, 0, "{text:?}");
        }
    }

    fn destination(id: &str) -> OscDestination {
        OscDestination {
            id: id.into(),
            name: id.into(),
            host: "127.0.0.1".into(),
            port: 9,
        }
    }

    fn track(id: &str, muted: bool, destination: Option<&str>, at: u64) -> Track {
        Track {
            audio_output: Default::default(),
            id: id.into(),
            name: id.into(),
            muted,
            gain_db: None,
            color: None,
            kind: TrackKind::Osc {
                destination: destination.map(Into::into),
                events: vec![OscEvent {
                    first_turn_only: false,
                    id: format!("e-{id}"),
                    at,
                    address: "/go".into(),
                    args: vec![],
                }],
            },
        }
    }

    fn timeline(tracks: Vec<Track>) -> Timeline {
        let mut timeline = Timeline::new("T", FrameRate::Fps25);
        timeline.tracks = tracks;
        timeline
    }

    #[test]
    fn plan_should_group_tracks_by_destination_and_skip_the_rest() {
        let t = timeline(vec![
            track("a", false, Some("d1"), 10),
            track("b", false, Some("d1"), 5),
            track("c", true, Some("d1"), 1),
            track("d", false, None, 1),
            track("e", false, Some("ghost"), 1),
            track("f", false, Some("d2"), 1),
        ]);
        let plans = plan(
            &t,
            &[destination("d1"), destination("d2")],
            &Clock::new(48_000, FrameRate::Fps25),
        );
        assert_eq!(plans.len(), 2);
        let times: Vec<u64> = plans[0].schedule.items().iter().map(|i| i.at).collect();
        assert_eq!(times, [5 * 1_920, 10 * 1_920]);
    }

    #[test]
    fn a_packet_should_reach_a_udp_socket_unchanged() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let destination = OscDestination {
            port: receiver.local_addr().unwrap().port(),
            ..destination("d1")
        };
        let mut sink = OscSink::connect(&destination).unwrap();
        let packet = encode("/cue/1/start", &[OscArg::Int(7)]);
        assert!(sink.send(&packet));
        let mut buffer = [0u8; 128];
        let len = receiver.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..len], &packet[..]);
    }

    #[test]
    fn connecting_to_a_host_that_does_not_resolve_should_fail_with_a_message() {
        let destination = OscDestination {
            host: "no such host.invalid".into(),
            ..destination("d1")
        };
        assert!(OscSink::connect(&destination).is_err());
    }
}
