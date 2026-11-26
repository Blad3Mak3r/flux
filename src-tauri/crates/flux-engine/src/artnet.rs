//! Art-Net input: the packets of a lighting console as DMX values a VJ mapping can follow.
//!
//! Everything here is pure, so it is tested without a network: [`parse_art_dmx`] reads one
//! packet, [`DmxState`] keeps the last frame of each universe and tells what changed in a new
//! one, [`heard`] turns the changes into what the mappings are the source of, and [`DmxLearn`]
//! chooses the channel the user moved while learning. The socket is in [`crate::control_in`].
//!
//! A console sends **every channel of a universe about 44 times a second**, moved or not, so only
//! a change is news: a fader that rests says nothing and a button that is held presses once.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use flux_project::vj_mapping::{DMX_CHANNELS, Heard, Source};

/// The UDP port of Art-Net.
pub const ARTNET_PORT: u16 = 6454;

const HEADER: &[u8; 8] = b"Art-Net\0";
const OP_DMX: u16 = 0x5000;
/// The first revision of the protocol that has the ArtDmx packet of today.
const MIN_VERSION: u16 = 14;
/// Bytes before the data of an ArtDmx packet.
const DMX_HEADER: usize = 18;
/// A DMX value from which a button counts as pressed.
const DOWN: u8 = 128;

/// An ArtDmx packet: the values of one universe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmxFrame {
    /// The 15-bit Port-Address: net, sub-net and universe.
    pub universe: u16,
    pub sequence: u8,
    /// The channels, 1 to 512 of them, the first being channel 1.
    pub data: Vec<u8>,
}

/// The ArtDmx packet in `packet`; `None` for anything else (another opcode, an old version, a
/// packet cut short, a length that is 0 or over 512), so a console's polls and replies are
/// ignored.
pub fn parse_art_dmx(packet: &[u8]) -> Option<DmxFrame> {
    if packet.len() < DMX_HEADER || &packet[..8] != HEADER {
        return None;
    }
    let opcode = u16::from_le_bytes([packet[8], packet[9]]);
    let version = u16::from_be_bytes([packet[10], packet[11]]);
    if opcode != OP_DMX || version < MIN_VERSION {
        return None;
    }
    let length = usize::from(u16::from_be_bytes([packet[16], packet[17]]));
    if length == 0 || length > usize::from(DMX_CHANNELS) {
        return None;
    }
    let data = packet.get(DMX_HEADER..DMX_HEADER + length)?;
    // SubUni is the low byte of the Port-Address, Net its high 7 bits.
    let universe = u16::from(packet[15] & 0x7F) << 8 | u16::from(packet[14]);
    Some(DmxFrame {
        universe,
        sequence: packet[12],
        data: data.to_vec(),
    })
}

/// A channel whose value is not what the last frame said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// From 1 to 512.
    pub channel: u16,
    pub old: u8,
    pub new: u8,
}

struct Universe {
    values: [u8; DMX_CHANNELS as usize],
    /// How many channels a frame of it has held, so channels it never sent are not "changes".
    seen: usize,
}

/// The last frame of each universe.
#[derive(Default)]
pub struct DmxState {
    universes: HashMap<u16, Universe>,
}

impl DmxState {
    /// Keeps `frame` and returns what it changed. The first frame of a universe (and the
    /// channels a longer frame adds) is only the state to compare with: a console that was
    /// already sending must not press every button the moment Flux starts to listen.
    pub fn update(&mut self, frame: &DmxFrame) -> Vec<Change> {
        let universe = self.universes.entry(frame.universe).or_insert(Universe {
            values: [0; DMX_CHANNELS as usize],
            seen: 0,
        });
        let mut changes = Vec::new();
        for (at, &new) in frame.data.iter().enumerate() {
            let old = universe.values[at];
            if at < universe.seen && old != new {
                changes.push(Change {
                    channel: at as u16 + 1,
                    old,
                    new,
                });
            }
            universe.values[at] = new;
        }
        universe.seen = universe.seen.max(frame.data.len());
        changes
    }

    /// The value of a channel in the last frame; `None` for a universe or channel not seen.
    pub fn value(&self, universe: u16, channel: u16) -> Option<u8> {
        let universe = self.universes.get(&universe)?;
        let at = usize::from(channel.checked_sub(1)?);
        (at < universe.seen).then(|| universe.values[at])
    }
}

/// What the Art-Net `sources` of the mappings heard from the `changes` of a frame of `universe`:
/// one [`Heard`] for each source that has a channel of the changes, its level 0 to 1 (a 16-bit
/// source reads its fine channel too) and whether it is down (128 and over, 32768 for 16 bits).
pub fn heard(
    state: &DmxState,
    universe: u16,
    changes: &[Change],
    sources: &[Source],
) -> Vec<Heard> {
    let moved = |channel: u16| changes.iter().any(|change| change.channel == channel);
    let mut out: Vec<Heard> = Vec::new();
    for source in sources {
        let Source::Artnet {
            universe: of,
            channel,
            fine,
        } = source
        else {
            continue;
        };
        if *of != universe || !(moved(*channel) || fine.is_some_and(moved)) {
            continue;
        }
        if out.iter().any(|heard| heard.source == *source) {
            continue;
        }
        let coarse = state.value(universe, *channel).unwrap_or(0);
        let (level, down) = match fine {
            Some(fine) => {
                let low = state.value(universe, *fine).unwrap_or(0);
                let wide = u16::from(coarse) << 8 | u16::from(low);
                (f32::from(wide) / 65535.0, coarse >= DOWN)
            }
            None => (f32::from(coarse) / 255.0, coarse >= DOWN),
        };
        out.push(Heard {
            source: source.clone(),
            level,
            down,
        });
    }
    out
}

/// How far a channel must have travelled (of 255) to count as the one the user moved: the
/// flicker of a dithering fixture or a noisy fader is smaller.
const MIN_TRAVEL: u8 = 10;
/// How long nothing moves for the learn to be decided.
const SETTLE: Duration = Duration::from_millis(350);
/// The longest the learn waits after the first channel qualified, for a control still moving.
const LONGEST: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy)]
struct Span {
    min: u8,
    max: u8,
    changes: u32,
}

/// Learning a DMX source: a console sends all its channels all the time, so "the next channel
/// heard" is no use. The channel the user moved is the one that **travelled furthest** (the span
/// between the lowest and the highest value it took, the one it had when learning began counted)
/// and moved more often when two travelled alike, then the lowest. It is decided once the
/// channels have rested for [`SETTLE`], or [`LONGEST`] after the first one qualified.
#[derive(Default)]
pub struct DmxLearn {
    spans: HashMap<(u16, u16), Span>,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl DmxLearn {
    /// Takes the changes of a frame of `universe` heard at `now`.
    pub fn observe(&mut self, universe: u16, changes: &[Change], now: Instant) {
        for change in changes {
            let span = self
                .spans
                .entry((universe, change.channel))
                .or_insert(Span {
                    min: change.old,
                    max: change.old,
                    changes: 0,
                });
            span.min = span.min.min(change.new);
            span.max = span.max.max(change.new);
            span.changes += 1;
            if span.max - span.min >= MIN_TRAVEL {
                self.first.get_or_insert(now);
                self.last = Some(now);
            }
        }
    }

    /// The channel that travelled furthest, if any travelled enough.
    fn best(&self) -> Option<(u16, u16)> {
        self.spans
            .iter()
            .filter(|(_, span)| span.max - span.min >= MIN_TRAVEL)
            .max_by_key(|(key, span)| (span.max - span.min, span.changes, std::cmp::Reverse(**key)))
            .map(|(key, _)| *key)
    }

    /// How long until the choice is due; `None` while no channel has qualified.
    pub fn due_in(&self, now: Instant) -> Option<Duration> {
        let (first, last) = (self.first?, self.last?);
        let at = (last + SETTLE).min(first + LONGEST);
        Some(at.saturating_duration_since(now))
    }

    /// The choice, once it is due; the learn starts over after it. Returns `(universe, channel)`.
    pub fn take(&mut self, now: Instant) -> Option<(u16, u16)> {
        if self.due_in(now)? > Duration::ZERO {
            return None;
        }
        let best = self.best();
        self.reset();
        best
    }

    /// Forgets what was seen: the learn was cancelled, or is over.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether anything was seen since the last reset.
    pub fn is_idle(&self) -> bool {
        self.spans.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ArtDmx packet as a console sends it.
    fn packet(net: u8, sub_uni: u8, data: &[u8]) -> Vec<u8> {
        let mut bytes = b"Art-Net\0".to_vec();
        bytes.extend([0x00, 0x50, 0, 14, 7, 0, sub_uni, net]);
        bytes.extend((data.len() as u16).to_be_bytes());
        bytes.extend(data);
        bytes
    }

    fn frame(universe: u16, data: &[u8]) -> DmxFrame {
        DmxFrame {
            universe,
            sequence: 0,
            data: data.to_vec(),
        }
    }

    /// A full 512-channel frame with `value` on `channel`.
    fn full(channel: u16, value: u8) -> DmxFrame {
        let mut data = vec![0; 512];
        data[usize::from(channel) - 1] = value;
        frame(1, &data)
    }

    fn at(ms: u64) -> Instant {
        static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *ORIGIN.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    #[test]
    fn a_valid_packet_should_give_its_port_address_sequence_and_data() {
        let parsed = parse_art_dmx(&packet(2, 0x31, &[10, 20, 30, 40])).unwrap();

        assert_eq!(parsed.universe, 0x0231, "net 2, sub-net 3, universe 1");
        assert_eq!(parsed.sequence, 7);
        assert_eq!(parsed.data, vec![10, 20, 30, 40]);
    }

    #[test]
    fn the_high_bit_of_the_net_byte_should_not_be_part_of_the_address() {
        assert_eq!(
            parse_art_dmx(&packet(0xFF, 0xFF, &[1, 2]))
                .unwrap()
                .universe,
            0x7FFF
        );
    }

    #[test]
    fn a_full_universe_should_parse() {
        assert_eq!(
            parse_art_dmx(&packet(0, 0, &[9; 512])).unwrap().data.len(),
            512
        );
    }

    #[test]
    fn short_or_broken_packets_should_be_ignored() {
        let good = packet(0, 1, &[1, 2, 3, 4]);
        assert!(parse_art_dmx(&[]).is_none());
        assert!(
            parse_art_dmx(&good[..17]).is_none(),
            "cut inside the header"
        );
        assert!(
            parse_art_dmx(&good[..good.len() - 1]).is_none(),
            "less data than it says"
        );
        assert!(parse_art_dmx(&packet(0, 1, &[])).is_none(), "no channels");
        assert!(
            parse_art_dmx(&packet(0, 1, &[0; 513])).is_none(),
            "too many channels"
        );
        let mut wrong_id = good.clone();
        wrong_id[0] = b'X';
        assert!(parse_art_dmx(&wrong_id).is_none());
        let mut old = good.clone();
        old[11] = 13;
        assert!(
            parse_art_dmx(&old).is_none(),
            "version 13 has no ArtDmx like this"
        );
    }

    #[test]
    fn other_opcodes_should_be_ignored() {
        let mut poll = packet(0, 1, &[1, 2]);
        poll[8..10].copy_from_slice(&0x2000_u16.to_le_bytes());
        assert!(parse_art_dmx(&poll).is_none());
        let mut sync = packet(0, 1, &[1, 2]);
        sync[8..10].copy_from_slice(&0x5200_u16.to_le_bytes());
        assert!(parse_art_dmx(&sync).is_none());
    }

    #[test]
    fn the_first_frame_should_be_the_state_and_change_nothing() {
        let mut state = DmxState::default();
        assert!(state.update(&full(37, 255)).is_empty());
        assert_eq!(state.value(1, 37), Some(255));
    }

    #[test]
    fn a_repeated_frame_should_change_nothing_and_a_moved_channel_only_itself() {
        let mut state = DmxState::default();
        state.update(&full(37, 10));

        assert!(state.update(&full(37, 10)).is_empty());
        assert_eq!(
            state.update(&full(37, 90)),
            vec![Change {
                channel: 37,
                old: 10,
                new: 90
            }]
        );
        assert!(state.update(&full(37, 90)).is_empty());
    }

    #[test]
    fn universes_should_be_kept_apart() {
        let mut state = DmxState::default();
        state.update(&frame(1, &[5, 5]));
        state.update(&frame(2, &[9, 9]));

        assert!(state.update(&frame(2, &[9, 9])).is_empty());
        assert_eq!(state.update(&frame(1, &[5, 6])).len(), 1);
        assert_eq!(state.value(2, 1), Some(9));
    }

    #[test]
    fn channels_a_longer_frame_adds_should_not_count_as_changes() {
        let mut state = DmxState::default();
        state.update(&frame(1, &[1, 2]));

        assert!(state.update(&frame(1, &[1, 2, 200, 200])).is_empty());
        assert_eq!(state.value(1, 3), Some(200));
    }

    fn source(channel: u16, fine: Option<u16>) -> Source {
        Source::Artnet {
            universe: 1,
            channel,
            fine,
        }
    }

    #[test]
    fn an_8_bit_source_should_read_the_value_over_255_and_press_from_128() {
        let mut state = DmxState::default();
        state.update(&full(5, 0));
        let sources = [source(5, None), source(6, None)];

        for (value, level, down) in [
            (255, 1.0, true),
            (128, 128.0 / 255.0, true),
            (127, 127.0 / 255.0, false),
        ] {
            let changes = state.update(&full(5, value));
            let heard = heard(&state, 1, &changes, &sources);
            assert_eq!(heard.len(), 1, "only the source of channel 5");
            assert_eq!(heard[0].level, level);
            assert_eq!(heard[0].down, down);
        }
    }

    #[test]
    fn a_16_bit_source_should_read_both_channels_and_follow_either() {
        let mut state = DmxState::default();
        let both = |hi: u8, lo: u8| {
            let mut data = vec![0; 512];
            data[36] = hi;
            data[37] = lo;
            frame(1, &data)
        };
        state.update(&both(0, 0));
        let sources = [source(37, Some(38))];

        let changes = state.update(&both(0x80, 0x00));
        let first = heard(&state, 1, &changes, &sources);
        assert_eq!(first[0].level, 32768.0 / 65535.0);
        assert!(first[0].down, "32768 and over is down");

        let changes = state.update(&both(0x80, 0xFF));
        let second = heard(&state, 1, &changes, &sources);
        assert_eq!(second.len(), 1, "a move of the fine channel alone is heard");
        assert_eq!(second[0].level, 33023.0 / 65535.0);

        let changes = state.update(&both(0, 0xFF));
        assert!(!heard(&state, 1, &changes, &sources)[0].down);
    }

    #[test]
    fn a_channel_of_another_universe_should_not_be_heard() {
        let mut state = DmxState::default();
        state.update(&full(5, 0));
        let changes = state.update(&full(5, 99));
        assert!(heard(&state, 2, &changes, &[source(5, None)]).is_empty());
    }

    #[test]
    fn the_same_source_twice_should_be_heard_once() {
        let mut state = DmxState::default();
        state.update(&full(5, 0));
        let changes = state.update(&full(5, 99));
        let sources = [source(5, None), source(5, None)];
        assert_eq!(heard(&state, 1, &changes, &sources).len(), 1);
    }

    /// Feeds a console's frames, all 512 channels each, one every 23 ms.
    fn console(
        state: &mut DmxState,
        learn: &mut DmxLearn,
        frames: &[Vec<(u16, u8)>],
        from_ms: u64,
    ) -> u64 {
        let mut ms = from_ms;
        for moved in frames {
            let mut data = vec![0; 512];
            for (channel, value) in moved {
                data[usize::from(*channel) - 1] = *value;
            }
            let changes = state.update(&frame(1, &data));
            learn.observe(1, &changes, at(ms));
            ms += 23;
        }
        ms
    }

    #[test]
    fn learning_should_pick_the_channel_that_travelled_furthest_among_resting_ones() {
        let mut state = DmxState::default();
        let mut learn = DmxLearn::default();
        let rest = vec![(3, 100), (10, 50), (20, 200)];
        let mut frames = vec![rest.clone(); 5];
        // Channel 37 is swept up while a fixture's channel 3 flickers by 2.
        frames.extend(
            (0..=10).map(|step| vec![(3, 100 + step % 3), (10, 50), (20, 200), (37, step * 25)]),
        );
        let end = console(&mut state, &mut learn, &frames, 0);

        assert_eq!(learn.take(at(end)), None, "still moving: not decided");
        assert_eq!(learn.take(at(end + 400)), Some((1, 37)));
        assert!(learn.is_idle(), "the learn starts over after the choice");
    }

    #[test]
    fn a_chase_that_flickers_a_little_should_not_beat_a_fader() {
        let mut state = DmxState::default();
        let mut learn = DmxLearn::default();
        let mut frames = vec![vec![(7, 100)]; 3];
        frames.extend((0..40).map(|n| vec![(7, 100 + (n % 2) * 6)]));
        frames.extend([
            vec![(7, 100), (40, 0)],
            vec![(7, 100), (40, 60)],
            vec![(7, 100), (40, 120)],
        ]);
        let end = console(&mut state, &mut learn, &frames, 0);

        assert_eq!(learn.take(at(end + 400)), Some((1, 40)));
    }

    #[test]
    fn a_button_should_be_learned_after_the_settle_time() {
        let mut state = DmxState::default();
        let mut learn = DmxLearn::default();
        let end = console(
            &mut state,
            &mut learn,
            &[vec![], vec![], vec![(12, 255)]],
            0,
        );

        assert_eq!(learn.take(at(end)), None);
        assert_eq!(learn.take(at(end + 400)), Some((1, 12)));
    }

    #[test]
    fn nothing_should_be_chosen_while_only_tiny_moves_were_seen() {
        let mut state = DmxState::default();
        let mut learn = DmxLearn::default();
        let end = console(
            &mut state,
            &mut learn,
            &[vec![(4, 100)], vec![(4, 103)], vec![(4, 100)]],
            0,
        );

        assert_eq!(learn.due_in(at(end)), None);
        assert_eq!(learn.take(at(end + 5000)), None);
    }

    #[test]
    fn a_control_that_never_rests_should_be_chosen_after_the_longest_wait() {
        let mut state = DmxState::default();
        let mut learn = DmxLearn::default();
        let frames: Vec<_> = (0..100).map(|n| vec![(9, (n * 2) as u8)]).collect();
        console(&mut state, &mut learn, &frames, 0);

        // 100 frames last 2.3 s: the longest wait passed while it still moved.
        assert_eq!(learn.take(at(2300)), Some((1, 9)));
    }

    #[test]
    fn when_two_channels_travel_alike_the_one_that_moved_more_often_wins() {
        let mut state = DmxState::default();
        let mut learn = DmxLearn::default();
        let end = console(
            &mut state,
            &mut learn,
            &[
                vec![(5, 0), (6, 0)],
                vec![(5, 100), (6, 50)],
                vec![(5, 100), (6, 100)],
            ],
            0,
        );

        assert_eq!(learn.take(at(end + 400)), Some((1, 6)));
    }
}
