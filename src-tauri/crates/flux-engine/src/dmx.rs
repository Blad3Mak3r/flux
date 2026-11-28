//! The DMX tracks: what the lights show at any position of the timeline, the Art-Net and sACN
//! packets that carry it, and the thread that sends them.
//!
//! The lights are a **state**, not a list of events, so a seek is exact: [`DmxPlan::frames`] folds
//! the scenes up to a position, each fading from what the lights showed when it started (a scene
//! that starts mid-fade of the one before takes over from where that had got to) and holding what
//! it does not name, unless it releases the others. Several tracks on one destination merge
//! channel by channel, the highest value winning.
//!
//! The thread follows the position the audio callback publishes, like the other cue threads. While
//! the transport plays it sends the state about 40 times a second when it changes; after a stop it
//! holds the last frame (a seek recomputes it), and it repeats the frame once a second so a node
//! that lost it recovers. Nothing here logs: a failed send is counted for the log lane to report.

use std::collections::BTreeMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use flux_core::Clock;
use flux_project::{DmxDestination, DmxProtocol, Timeline, TrackKind, UNIVERSE_SIZE};

use super::artnet::ARTNET_PORT;
use super::output::SharedClock;
use super::transport::TransportState;

/// The UDP port of sACN (E1.31).
pub const SACN_PORT: u16 = 5568;
/// How often the thread looks at the transport: 40 frames a second.
const TICK: Duration = Duration::from_millis(25);
/// A frame that has not changed is still sent this often.
const KEEPALIVE_NANOS: u64 = 1_000_000_000;
const SLOTS: usize = UNIVERSE_SIZE as usize;

/// The 512 values of a universe.
pub type Universe = [u8; SLOTS];

/// What a destination is sent: the values of each universe the plan writes to.
pub type Frames = BTreeMap<u16, Universe>;

type Key = (u16, u16);

/// A scene with its times in samples.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedScene {
    pub at: u64,
    pub fade: u64,
    pub release_others: bool,
    pub channels: Vec<(Key, u8)>,
}

/// Where one channel is going: from `from` at `start` to `to` over `fade` samples.
#[derive(Clone, Copy)]
struct Segment {
    from: f64,
    to: f64,
    start: u64,
    fade: u64,
}

impl Segment {
    fn at(&self, position: u64) -> f64 {
        if self.fade == 0 {
            return self.to;
        }
        let progress = (position.saturating_sub(self.start) as f64 / self.fade as f64).min(1.0);
        self.from + (self.to - self.from) * progress
    }
}

/// The values at `position` of the channels the scenes of one track set. A scene moves the
/// channels it names from where each is at its start, and each channel carries on with its own
/// fade when a later scene does not name it.
fn track_state(scenes: &[PlannedScene], position: u64) -> BTreeMap<Key, u8> {
    let mut channels: BTreeMap<Key, Segment> = BTreeMap::new();
    for scene in scenes.iter().take_while(|scene| scene.at <= position) {
        let go = |channels: &mut BTreeMap<Key, Segment>, key: Key, to: f64| {
            let from = channels
                .get(&key)
                .map_or(0.0, |segment| segment.at(scene.at));
            channels.insert(
                key,
                Segment {
                    from,
                    to,
                    start: scene.at,
                    fade: scene.fade,
                },
            );
        };
        if scene.release_others {
            let others: Vec<Key> = channels
                .keys()
                .filter(|key| !scene.channels.iter().any(|(named, _)| named == *key))
                .copied()
                .collect();
            for key in others {
                go(&mut channels, key, 0.0);
            }
        }
        for &(key, value) in &scene.channels {
            go(&mut channels, key, f64::from(value));
        }
    }
    channels
        .into_iter()
        .map(|(key, segment)| (key, segment.at(position).round().clamp(0.0, 255.0) as u8))
        .collect()
}

/// What one destination is sent.
#[derive(Debug, Clone, PartialEq)]
pub struct DmxPlan {
    pub destination: DmxDestination,
    /// One list of scenes, in time order, for each unmuted track that sends here.
    pub tracks: Vec<Vec<PlannedScene>>,
}

impl DmxPlan {
    /// The values of every universe the plan writes to, at `position` samples. A channel no scene
    /// has set yet is 0.
    pub fn frames(&self, position: u64) -> Frames {
        let mut frames = Frames::new();
        for scenes in &self.tracks {
            for scene in scenes {
                for &((universe, _), _) in &scene.channels {
                    frames.entry(universe).or_insert([0; SLOTS]);
                }
            }
            for ((universe, channel), value) in track_state(scenes, position) {
                let slot =
                    &mut frames.entry(universe).or_insert([0; SLOTS])[usize::from(channel) - 1];
                *slot = (*slot).max(value);
            }
        }
        frames
    }
}

/// The plan of every destination the unmuted DMX tracks of `timeline` send to. A track whose
/// destination is not in `destinations` (or has none) sends nothing.
pub fn plan(timeline: &Timeline, destinations: &[DmxDestination], clock: &Clock) -> Vec<DmxPlan> {
    let mut plans: Vec<DmxPlan> = Vec::new();
    for track in timeline.tracks.iter().filter(|track| !track.muted) {
        let TrackKind::Dmx {
            destination: Some(id),
            scenes,
        } = &track.kind
        else {
            continue;
        };
        let Some(destination) = destinations.iter().find(|d| &d.id == id) else {
            continue;
        };
        let mut planned: Vec<PlannedScene> = scenes
            .iter()
            .map(|scene| PlannedScene {
                at: clock.frames_to_samples(scene.at),
                fade: clock.frames_to_samples(scene.fade),
                release_others: scene.release_others,
                channels: scene
                    .channels
                    .iter()
                    .map(|c| ((c.universe, c.channel), c.value))
                    .collect(),
            })
            .collect();
        planned.sort_by_key(|scene| scene.at);
        match plans.iter_mut().find(|plan| plan.destination.id == *id) {
            Some(plan) => plan.tracks.push(planned),
            None => plans.push(DmxPlan {
                destination: destination.clone(),
                tracks: vec![planned],
            }),
        }
    }
    plans
}

/// An ArtDmx packet: `data` (padded to an even length, as the protocol wants) for `universe`.
pub fn encode_artnet(sequence: u8, universe: u16, data: &[u8]) -> Vec<u8> {
    let length = data.len().min(SLOTS).next_multiple_of(2).max(2);
    let mut out = Vec::with_capacity(18 + length);
    out.extend_from_slice(b"Art-Net\0");
    out.extend_from_slice(&0x5000u16.to_le_bytes());
    out.extend_from_slice(&14u16.to_be_bytes());
    out.push(sequence);
    out.push(0); // Physical
    out.push((universe & 0xFF) as u8); // SubUni
    out.push(((universe >> 8) & 0x7F) as u8); // Net
    out.extend_from_slice(&(length as u16).to_be_bytes());
    out.extend_from_slice(&data[..data.len().min(SLOTS)]);
    out.resize(18 + length, 0);
    out
}

const SACN_NAME_LEN: usize = 64;
const SACN_PRIORITY: u8 = 100;

/// An E1.31 data packet for sACN `universe` (from 1), with the start code 0 and `data` as slots.
pub fn encode_sacn(
    cid: &[u8; 16],
    name: &str,
    sequence: u8,
    universe: u16,
    data: &[u8],
) -> Vec<u8> {
    let slots = data.len().min(SLOTS);
    let total = 126 + slots;
    let flags = |from: usize| (0x7000 | (total - from) as u16).to_be_bytes();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&0x0010u16.to_be_bytes()); // preamble size
    out.extend_from_slice(&0u16.to_be_bytes()); // postamble size
    out.extend_from_slice(b"ASC-E1.17\0\0\0");
    out.extend_from_slice(&flags(16));
    out.extend_from_slice(&4u32.to_be_bytes()); // root vector: data
    out.extend_from_slice(cid);
    out.extend_from_slice(&flags(38));
    out.extend_from_slice(&2u32.to_be_bytes()); // framing vector: data packet
    let mut source = [0u8; SACN_NAME_LEN];
    let bytes = name.as_bytes();
    let used = bytes.len().min(SACN_NAME_LEN - 1);
    source[..used].copy_from_slice(&bytes[..used]);
    out.extend_from_slice(&source);
    out.push(SACN_PRIORITY);
    out.extend_from_slice(&0u16.to_be_bytes()); // synchronisation address
    out.push(sequence);
    out.push(0); // options
    out.extend_from_slice(&universe.to_be_bytes());
    out.extend_from_slice(&flags(115));
    out.push(0x02); // DMP vector: set property
    out.push(0xA1); // address and data type
    out.extend_from_slice(&0u16.to_be_bytes()); // first property address
    out.extend_from_slice(&1u16.to_be_bytes()); // address increment
    out.extend_from_slice(&(slots as u16 + 1).to_be_bytes()); // start code and slots
    out.push(0); // start code
    out.extend_from_slice(&data[..slots]);
    out
}

/// A UDP socket that sends DMX to a destination, in the destination's protocol.
pub struct DmxSink {
    socket: UdpSocket,
    address: SocketAddr,
    protocol: DmxProtocol,
    cid: [u8; 16],
    sequences: BTreeMap<u16, u8>,
}

impl DmxSink {
    /// Resolves the destination's host and opens a socket for it. It can wait on a name lookup, so
    /// it is for the control thread, never for the one that sends.
    pub fn connect(destination: &DmxDestination) -> Result<Self, String> {
        let port = match destination.protocol {
            DmxProtocol::Artnet => ARTNET_PORT,
            DmxProtocol::Sacn => SACN_PORT,
        };
        Self::connect_to(destination, port)
    }

    /// As [`connect`](Self::connect), on a UDP port other than the protocol's own.
    pub(crate) fn connect_to(destination: &DmxDestination, port: u16) -> Result<Self, String> {
        let address = (destination.host.as_str(), port)
            .to_socket_addrs()
            .map_err(|err| format!("{}: {err}", destination.host))?
            .find(SocketAddr::is_ipv4)
            .ok_or_else(|| format!("{}: no IPv4 address", destination.host))?;
        let socket =
            UdpSocket::bind("0.0.0.0:0").map_err(|err| format!("{}: {err}", destination.host))?;
        // Art-Net is often broadcast; a full send buffer must drop the frame, not hold the thread.
        socket
            .set_broadcast(true)
            .and_then(|()| socket.set_nonblocking(true))
            .map_err(|err| err.to_string())?;
        Ok(Self {
            socket,
            address,
            protocol: destination.protocol,
            cid: source_id(),
            sequences: BTreeMap::new(),
        })
    }

    /// Sends the values of `universe`. `false` means they could not be sent.
    pub fn send(&mut self, universe: u16, data: &Universe) -> bool {
        let sequence = self.sequences.entry(universe).or_insert(0);
        // Art-Net numbers from 1 (0 turns the sequence off); sACN wraps through 0.
        *sequence = match self.protocol {
            DmxProtocol::Artnet => sequence.checked_add(1).unwrap_or(1),
            DmxProtocol::Sacn => sequence.wrapping_add(1),
        };
        let packet = match self.protocol {
            DmxProtocol::Artnet => encode_artnet(*sequence, universe, data),
            DmxProtocol::Sacn => encode_sacn(&self.cid, "FLUX", *sequence, universe + 1, data),
        };
        self.socket.send_to(&packet, self.address).is_ok()
    }
}

/// An identifier for this sender, random for each socket, as sACN asks of a source.
fn source_id() -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut id = [0u8; 16];
    let (low, high) = id.split_at_mut(8);
    for half in [low, high] {
        half.copy_from_slice(&RandomState::new().build_hasher().finish().to_le_bytes());
    }
    id[6] = (id[6] & 0x0F) | 0x40; // a version 4 UUID, as the spec recommends
    id[8] = (id[8] & 0x3F) | 0x80;
    id
}

/// A plan that can be swapped while the thread that sends it runs.
#[derive(Debug)]
pub struct SharedPlan {
    current: Mutex<Arc<DmxPlan>>,
    version: AtomicU64,
}

impl SharedPlan {
    pub fn new(plan: DmxPlan) -> Self {
        Self {
            current: Mutex::new(Arc::new(plan)),
            version: AtomicU64::new(0),
        }
    }

    pub fn replace(&self, plan: DmxPlan) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(plan);
        self.version.fetch_add(1, Ordering::Release);
    }

    /// Leaves the destination with no scenes: it goes quiet.
    pub fn clear(&self) {
        self.replace(DmxPlan {
            destination: self.load().destination.clone(),
            tracks: Vec::new(),
        });
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    pub fn load(&self) -> Arc<DmxPlan> {
        Arc::clone(&self.current.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Decides, tick by tick, what the thread sends. Pure, so it is tested without a clock or socket.
struct Follower {
    plan: Arc<DmxPlan>,
    epoch: Option<u64>,
    state: TransportState,
    /// A stop leaves the lights as they were, until the transport moves again.
    held: bool,
    frames: Frames,
    dirty: bool,
    last_sent: Option<u64>,
}

impl Follower {
    fn new(plan: Arc<DmxPlan>) -> Self {
        Self {
            plan,
            epoch: None,
            state: TransportState::Stopped,
            held: false,
            frames: Frames::new(),
            dirty: false,
            last_sent: None,
        }
    }

    /// Carries on with an edited plan (recomputed at the next step, unless a stop is held).
    fn replace(&mut self, plan: Arc<DmxPlan>) {
        self.plan = plan;
        self.dirty = true;
    }

    /// The frames to send now, if any. `now` is in nanoseconds.
    fn step(
        &mut self,
        state: TransportState,
        epoch: u64,
        position: u64,
        now: u64,
    ) -> Option<&Frames> {
        let moved = self.epoch != Some(epoch);
        if moved {
            self.epoch = Some(epoch);
            // A stop (not a seek while stopped) keeps what the lights show.
            self.held = state == TransportState::Stopped && self.state != TransportState::Stopped;
        }
        if state == TransportState::Playing {
            self.held = false;
        }
        self.state = state;
        if (moved || self.dirty || state == TransportState::Playing) && !self.held {
            let next = self.plan.frames(position);
            if next != self.frames {
                self.frames = next;
                self.dirty = true;
            }
        }
        let due = self
            .last_sent
            .is_none_or(|sent| now.saturating_sub(sent) >= KEEPALIVE_NANOS);
        if (self.dirty || due) && !self.frames.is_empty() {
            self.dirty = false;
            self.last_sent = Some(now);
            Some(&self.frames)
        } else {
            self.dirty = false;
            None
        }
    }
}

/// Sends `plan` into `sink` as the transport that `shared` publishes moves, until `stop`.
pub(crate) fn run_dmx(
    plan: &SharedPlan,
    clock: Clock,
    mut sink: DmxSink,
    shared: &SharedClock,
    stop: &AtomicBool,
) {
    let mut version = plan.version();
    let mut follower = Follower::new(plan.load());
    while !stop.load(Ordering::Relaxed) {
        if plan.version() != version {
            version = plan.version();
            follower.replace(plan.load());
        }
        let snapshot = shared.load();
        let now = shared.now();
        let position = snapshot.estimated_position(now, clock);
        if let Some(frames) = follower.step(snapshot.state, snapshot.epoch, position, now) {
            for (&universe, data) in frames {
                if !sink.send(universe, data) {
                    shared.cue_failed();
                }
            }
        }
        thread::park_timeout(TICK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flux_core::FrameRate;
    use flux_project::{DmxChannel, DmxScene, Track};

    fn scene(at: u64, fade: u64, release: bool, channels: &[(u16, u16, u8)]) -> PlannedScene {
        PlannedScene {
            at,
            fade,
            release_others: release,
            channels: channels.iter().map(|&(u, c, v)| ((u, c), v)).collect(),
        }
    }

    fn value(scenes: &[PlannedScene], position: u64, universe: u16, channel: u16) -> u8 {
        *track_state(scenes, position)
            .get(&(universe, channel))
            .unwrap_or(&0)
    }

    #[test]
    fn nothing_is_set_before_the_first_scene() {
        let scenes = [scene(100, 0, false, &[(0, 1, 200)])];
        assert!(track_state(&scenes, 99).is_empty());
        assert_eq!(value(&scenes, 100, 0, 1), 200);
    }

    #[test]
    fn a_fade_should_be_a_straight_line_from_the_current_value() {
        let scenes = [
            scene(0, 0, false, &[(0, 1, 100)]),
            scene(1_000, 1_000, false, &[(0, 1, 200)]),
        ];
        assert_eq!(value(&scenes, 999, 0, 1), 100);
        assert_eq!(value(&scenes, 1_000, 0, 1), 100);
        assert_eq!(value(&scenes, 1_500, 0, 1), 150);
        assert_eq!(value(&scenes, 2_000, 0, 1), 200);
        assert_eq!(value(&scenes, 9_999, 0, 1), 200);
    }

    #[test]
    fn channels_a_scene_does_not_name_should_hold_their_value() {
        let scenes = [
            scene(0, 0, false, &[(0, 1, 100), (0, 2, 50)]),
            scene(1_000, 0, false, &[(0, 1, 10)]),
        ];
        assert_eq!(value(&scenes, 1_000, 0, 1), 10);
        assert_eq!(value(&scenes, 1_000, 0, 2), 50);
    }

    #[test]
    fn a_scene_that_releases_the_others_should_fade_them_to_zero() {
        let scenes = [
            scene(0, 0, false, &[(0, 1, 100), (0, 2, 50), (1, 1, 80)]),
            scene(1_000, 1_000, true, &[(0, 1, 100)]),
        ];
        assert_eq!(value(&scenes, 1_500, 0, 2), 25);
        assert_eq!(value(&scenes, 1_500, 0, 1), 100);
        assert_eq!(value(&scenes, 2_000, 0, 2), 0);
        assert_eq!(value(&scenes, 2_000, 1, 1), 0);
    }

    #[test]
    fn a_scene_that_starts_mid_fade_should_take_over_from_where_the_fade_had_got() {
        let scenes = [
            scene(0, 0, false, &[(0, 1, 0)]),
            scene(1_000, 1_000, false, &[(0, 1, 200)]),
            scene(1_500, 0, false, &[(0, 2, 7)]),
            scene(2_000, 500, false, &[(0, 1, 0)]),
        ];
        // The third scene does not name channel 1, so its fade towards 200 carries on.
        assert_eq!(value(&scenes, 1_500, 0, 1), 100);
        assert_eq!(value(&scenes, 1_750, 0, 1), 150);
        assert_eq!(value(&scenes, 2_000, 0, 2), 7);
        assert_eq!(value(&scenes, 2_250, 0, 1), 100);
        assert_eq!(value(&scenes, 2_500, 0, 1), 0);
    }

    #[test]
    fn seeking_anywhere_should_give_the_same_state_as_playing_up_to_there() {
        let scenes = [
            scene(0, 0, false, &[(0, 1, 255)]),
            scene(480, 960, true, &[(0, 3, 90)]),
        ];
        let at_once = track_state(&scenes, 960);
        assert_eq!(at_once.get(&(0, 1)), Some(&128));
        assert_eq!(at_once.get(&(0, 3)), Some(&45));
    }

    fn plan_of(tracks: Vec<Vec<PlannedScene>>) -> DmxPlan {
        DmxPlan {
            destination: DmxDestination {
                id: "d".into(),
                name: "d".into(),
                protocol: DmxProtocol::Artnet,
                host: "127.0.0.1".into(),
            },
            tracks,
        }
    }

    #[test]
    fn tracks_on_one_destination_should_merge_with_the_highest_value_winning() {
        let plan = plan_of(vec![
            vec![scene(0, 0, false, &[(0, 1, 40), (0, 2, 200)])],
            vec![scene(0, 0, false, &[(0, 1, 90), (2, 1, 5)])],
        ]);
        let frames = plan.frames(10);
        assert_eq!(frames[&0][0], 90);
        assert_eq!(frames[&0][1], 200);
        assert_eq!(frames[&2][0], 5);
        assert_eq!(frames.len(), 2);
    }

    #[test]
    fn the_plan_should_skip_muted_tracks_and_unknown_destinations_and_convert_to_samples() {
        let clock = Clock::new(48_000, FrameRate::Fps25);
        let destination = plan_of(Vec::new()).destination;
        let track = |id: &str, muted: bool, to: Option<&str>| Track {
            audio_output: Default::default(),
            id: id.into(),
            name: id.into(),
            muted,
            gain_db: None,
            color: None,
            kind: TrackKind::Dmx {
                destination: to.map(Into::into),
                scenes: vec![DmxScene {
                    id: format!("s-{id}"),
                    at: 5,
                    name: String::new(),
                    fade: 25,
                    release_others: false,
                    channels: vec![DmxChannel {
                        universe: 0,
                        channel: 1,
                        value: 9,
                    }],
                }],
            },
        };
        let mut timeline = Timeline::new("T", FrameRate::Fps25);
        timeline.tracks = vec![
            track("a", false, Some("d")),
            track("b", true, Some("d")),
            track("c", false, None),
            track("e", false, Some("ghost")),
        ];
        let plans = plan(&timeline, &[destination], &clock);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].tracks.len(), 1);
        assert_eq!(plans[0].tracks[0][0].at, 5 * 1_920);
        assert_eq!(plans[0].tracks[0][0].fade, 48_000);
    }

    #[test]
    fn an_artdmx_packet_should_have_the_header_opcode_sequence_universe_and_an_even_length() {
        let packet = encode_artnet(7, 0x1234, &[1, 2, 3]);
        assert_eq!(&packet[..8], b"Art-Net\0");
        assert_eq!(
            &packet[8..10],
            [0x00, 0x50],
            "OpCode 0x5000, low byte first"
        );
        assert_eq!(&packet[10..12], [0, 14], "protocol version 14");
        assert_eq!(packet[12], 7);
        assert_eq!(packet[13], 0);
        assert_eq!(packet[14], 0x34, "SubUni");
        assert_eq!(packet[15], 0x12, "Net");
        assert_eq!(&packet[16..18], [0, 4], "padded to an even length");
        assert_eq!(&packet[18..], [1, 2, 3, 0]);
        let full = encode_artnet(1, 0, &[255; 512]);
        assert_eq!(full.len(), 18 + 512);
        assert_eq!(&full[16..18], [2, 0]);
    }

    #[test]
    fn the_artnet_packet_should_be_read_back_by_the_input_parser() {
        let mut data = [0u8; 512];
        data[9] = 77;
        let packet = encode_artnet(3, 5, &data);
        let frame = crate::artnet::parse_art_dmx(&packet).unwrap();
        assert_eq!((frame.universe, frame.sequence, frame.data[9]), (5, 3, 77));
        assert_eq!(frame.data.len(), 512);
    }

    #[test]
    fn an_sacn_packet_should_follow_the_e131_layout() {
        let cid = [9u8; 16];
        let packet = encode_sacn(&cid, "FLUX", 4, 2, &[10, 20]);
        assert_eq!(packet.len(), 126 + 2);
        assert_eq!(&packet[..4], [0, 0x10, 0, 0]);
        assert_eq!(&packet[4..16], b"ASC-E1.17\0\0\0");
        assert_eq!(
            &packet[16..18],
            (0x7000u16 | (128 - 16) as u16).to_be_bytes()
        );
        assert_eq!(&packet[18..22], [0, 0, 0, 4]);
        assert_eq!(&packet[22..38], cid);
        assert_eq!(
            &packet[38..40],
            (0x7000u16 | (128 - 38) as u16).to_be_bytes()
        );
        assert_eq!(&packet[40..44], [0, 0, 0, 2]);
        assert_eq!(&packet[44..48], b"FLUX");
        assert_eq!(packet[108], 100, "priority");
        assert_eq!(packet[111], 4, "sequence");
        assert_eq!(&packet[113..115], [0, 2], "universe");
        assert_eq!(
            &packet[115..117],
            (0x7000u16 | (128 - 115) as u16).to_be_bytes()
        );
        assert_eq!(packet[117], 0x02);
        assert_eq!(packet[118], 0xA1);
        assert_eq!(&packet[123..125], [0, 3], "start code and two slots");
        assert_eq!(&packet[125..], [0, 10, 20]);
        assert_eq!(encode_sacn(&cid, "FLUX", 0, 1, &[0; 512]).len(), 638);
    }

    fn destination(protocol: DmxProtocol) -> DmxDestination {
        DmxDestination {
            protocol,
            ..plan_of(Vec::new()).destination
        }
    }

    #[test]
    fn a_connection_to_a_host_that_does_not_resolve_should_fail_with_a_message() {
        let destination = DmxDestination {
            host: "no such host.invalid".into(),
            ..destination(DmxProtocol::Artnet)
        };
        assert!(DmxSink::connect(&destination).is_err());
    }

    #[test]
    fn the_sink_should_number_its_packets_and_reach_a_loopback_socket() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let port = receiver.local_addr().unwrap().port();
        let mut sink = DmxSink::connect_to(&destination(DmxProtocol::Artnet), port).unwrap();
        let mut data = [0u8; 512];
        data[0] = 42;
        assert!(sink.send(3, &data));
        assert!(sink.send(3, &data));
        let mut buffer = [0u8; 1024];
        let mut sequences = Vec::new();
        for _ in 0..2 {
            let len = receiver.recv(&mut buffer).unwrap();
            let frame = crate::artnet::parse_art_dmx(&buffer[..len]).unwrap();
            assert_eq!((frame.universe, frame.data[0]), (3, 42));
            sequences.push(frame.sequence);
        }
        assert_eq!(sequences, [1, 2]);
    }

    fn follower() -> Follower {
        let plan = plan_of(vec![vec![
            scene(0, 0, false, &[(0, 1, 100)]),
            scene(1_000, 0, false, &[(0, 1, 200)]),
        ]]);
        Follower::new(Arc::new(plan))
    }

    fn sent(
        follower: &mut Follower,
        state: TransportState,
        epoch: u64,
        position: u64,
        now: u64,
    ) -> Option<u8> {
        follower
            .step(state, epoch, position, now)
            .map(|frames| frames[&0][0])
    }

    #[test]
    fn the_thread_should_follow_play_seek_and_hold_a_stop() {
        use TransportState::*;
        let mut f = follower();
        // Opened while stopped at the start: the lights take the state there.
        assert_eq!(sent(&mut f, Stopped, 1, 0, 0), Some(100));
        assert_eq!(sent(&mut f, Stopped, 1, 0, 10), None);
        // Playing: a change is sent as soon as it happens.
        assert_eq!(sent(&mut f, Playing, 2, 0, 20), None);
        assert_eq!(sent(&mut f, Playing, 2, 1_000, 30), Some(200));
        // A stop holds the last frame, whatever position the transport goes back to.
        assert_eq!(sent(&mut f, Stopped, 3, 0, 40), None);
        assert_eq!(sent(&mut f, Stopped, 3, 0, 50), None);
        // The held frame is repeated once a second.
        assert_eq!(sent(&mut f, Stopped, 3, 0, 1_100_000_000), Some(200));
        // A seek while stopped puts the lights as they are at the new position.
        assert_eq!(sent(&mut f, Stopped, 4, 500, 1_100_000_010), Some(100));
        // So does a seek while playing.
        assert_eq!(sent(&mut f, Playing, 5, 1_200, 1_100_000_020), Some(200));
        assert_eq!(sent(&mut f, Playing, 6, 10, 1_100_000_030), Some(100));
    }

    #[test]
    fn pausing_should_hold_the_frame_and_an_edit_should_recompute_it() {
        use TransportState::*;
        let mut f = follower();
        assert_eq!(sent(&mut f, Playing, 1, 1_200, 0), Some(200));
        assert_eq!(sent(&mut f, Paused, 1, 1_200, 10), None);
        f.replace(Arc::new(plan_of(vec![vec![scene(
            0,
            0,
            false,
            &[(0, 1, 9)],
        )]])));
        assert_eq!(sent(&mut f, Paused, 1, 1_200, 20), Some(9));
    }
}
