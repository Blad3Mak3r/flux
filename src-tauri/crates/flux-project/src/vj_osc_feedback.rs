//! OSC feedback of a VJ show, as Resolume and QLab's OSC v1 send it: the state of the grid is
//! echoed to the OSC destinations the show names, on the same addresses that control it (see
//! [`crate::vj_mapping::osc_command`]), with the new value.
//!
//! Everything here is pure. [`messages`] reads the grid and what plays and says what every address
//! should now hold; [`Throttle`] keeps what was sent and lets through only what changed, at most
//! [`MIN_INTERVAL`] apart per address. The app (`src-tauri/src/vj_osc_feedback.rs`) owns the
//! sockets and the thread that sends: nothing is sent from an audio callback.

use std::collections::HashMap;
use std::time::Duration;

use crate::vj::{MAX_TRANSITION, Vj};
use crate::vj_feedback::PadState;
use crate::vj_mapping::{Class, Source, Target};
use crate::{ProjectError, Result};

/// The most destinations a show sends feedback to.
pub const MAX_DESTINATIONS: usize = 4;

/// The least time between two messages to the same address: at most 30 a second.
pub const MIN_INTERVAL: Duration = Duration::from_millis(34);

/// An argument of a feedback message: a switch or a connection is an integer, a level a float.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
}

impl Value {
    /// Whether `other` is worth sending after `self`: a different integer, or a level that moved.
    fn differs(self, other: Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a != b,
            (Self::Float(a), Self::Float(b)) => (a - b).abs() > 1e-4,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub address: String,
    pub value: Value,
}

/// What the grid is doing, which the project does not say.
pub struct Live<'a> {
    /// Whether a clip plays.
    pub playing: &'a dyn Fn(&str) -> bool,
    /// The column launched last, while nothing was launched or stopped by hand since.
    pub column: Option<&'a str>,
    /// 0 to 1.
    pub crossfader: f32,
    pub bpm: f64,
    /// Whether the beats are held.
    pub paused: bool,
}

/// The destinations of `text`, `host:port` separated by commas or spaces, as they are kept
/// (`host:port` each, no repeats); an empty text is none.
pub fn parse_destinations(text: &str) -> Result<Vec<String>> {
    let bad = |what: &str| ProjectError::Invalid(format!("OSC feedback: {what}"));
    let mut out: Vec<String> = Vec::new();
    for part in text.split([',', ' ', ';', '\n']).filter(|p| !p.is_empty()) {
        let (host, port) = part
            .rsplit_once(':')
            .ok_or_else(|| bad("a destination is host:port"))?;
        let host = host.trim_matches(['[', ']']);
        let port: u16 = port.parse().map_err(|_| bad("the port is 1 to 65535"))?;
        if host.is_empty() || port == 0 || host.chars().any(char::is_whitespace) {
            return Err(bad("a destination is host:port"));
        }
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        let one = format!("{host}:{port}");
        if !out.contains(&one) {
            out.push(one);
        }
    }
    if out.len() > MAX_DESTINATIONS {
        return Err(bad("at most 4 destinations"));
    }
    Ok(out)
}

/// The host and port of a kept destination.
pub fn split_destination(destination: &str) -> Option<(&str, u16)> {
    let (host, port) = destination.rsplit_once(':')?;
    Some((host.trim_matches(['[', ']']), port.parse().ok()?))
}

fn level(value: f32) -> Value {
    Value::Float(value.clamp(0.0, 1.0))
}

fn connected(on: bool) -> Value {
    Value::Int(i32::from(on))
}

/// The value of a continuous `target` in its own units, `None` when the grid does not show it.
fn current(vj: &Vj, live: &Live, target: &Target) -> Option<f32> {
    let layer = |id: &str| vj.layers.iter().find(|l| l.id == id);
    match target {
        Target::LayerMaster { layer: id } => layer(id).map(|l| l.master),
        Target::LayerVolume { layer: id } => layer(id).map(|l| l.volume),
        Target::LayerOpacity { layer: id } => layer(id).map(|l| l.opacity),
        Target::LayerTransition { layer: id } => layer(id).map(|l| l.transition),
        Target::GroupMaster { group } => vj.group(group).map(|g| g.master),
        Target::GroupOpacity { group } => vj.group(group).map(|g| g.opacity),
        Target::Crossfader => Some(live.crossfader),
        Target::Bpm => Some(live.bpm as f32),
        Target::Dashboard { knob } => Some(vj.dashboard.get(*knob).map_or(0.0, |k| k.value)),
        _ => None,
    }
}

/// What a learned OSC mapping echoes: a level through the mapping's own range, a switch or a
/// connection as 0 or 1.
fn learned(vj: &Vj, live: &Live, mapping: &crate::vj_mapping::Mapping) -> Option<Value> {
    if mapping.parked {
        return None;
    }
    match mapping.target.class() {
        Class::Continuous => {
            let value = current(vj, live, &mapping.target)?;
            let span = mapping.max - mapping.min;
            if span == 0.0 {
                return None;
            }
            let s = ((value - mapping.min) / span).clamp(0.0, 1.0);
            Some(level(if mapping.invert { 1.0 - s } else { s }))
        }
        _ => match crate::vj_feedback::state(vj, &mapping.target, live.playing, live.paused)? {
            PadState::Playing | PadState::On => Some(connected(true)),
            _ => Some(connected(false)),
        },
    }
}

/// Every address of the show with what it holds now: the addresses Resolume has by default
/// (clip and column connections, the layers' Master, Audio, Video and Transition faders, the
/// crossfader, the tempo and the dashboard's knobs), then the OSC addresses learned in the
/// mappings, which echo their target (an effect's setting has none).
pub fn messages(vj: &Vj, live: &Live) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::new();
    let mut push = |address: String, value: Value| out.push(Message { address, value });
    if let Some(deck) = vj.shown_deck() {
        for (n, layer) in vj.layers.iter().enumerate() {
            let n = n + 1;
            for clip in &layer.clips {
                let at = if layer.global {
                    clip.slot
                } else {
                    deck.columns
                        .iter()
                        .position(|c| clip.column.as_deref() == Some(&c.id))
                };
                if let Some(m) = at {
                    push(
                        format!("/composition/layers/{n}/clips/{}/connect", m + 1),
                        connected((live.playing)(&clip.id)),
                    );
                    for (k, knob) in clip.dashboard.iter().enumerate() {
                        push(
                            format!(
                                "/composition/layers/{n}/clips/{}/dashboard/link{}",
                                m + 1,
                                k + 1
                            ),
                            level(knob.value),
                        );
                    }
                }
            }
        }
        for (m, column) in deck.columns.iter().enumerate() {
            push(
                format!("/composition/columns/{}/connect", m + 1),
                connected(live.column == Some(column.id.as_str())),
            );
        }
    }
    for (n, layer) in vj.layers.iter().enumerate() {
        let n = n + 1;
        push(
            format!("/composition/layers/{n}/master"),
            level(layer.master),
        );
        push(
            format!("/composition/layers/{n}/audio/volume"),
            level(layer.volume),
        );
        push(
            format!("/composition/layers/{n}/video/opacity"),
            level(layer.opacity),
        );
        push(
            format!("/composition/layers/{n}/transition/duration"),
            level(layer.transition / MAX_TRANSITION),
        );
        for (k, knob) in layer.dashboard.iter().enumerate() {
            push(
                format!("/composition/layers/{n}/dashboard/link{}", k + 1),
                level(knob.value),
            );
        }
    }
    push(
        "/composition/crossfader/phase".into(),
        level(live.crossfader),
    );
    push(
        "/composition/tempocontroller/tempo".into(),
        Value::Float(live.bpm as f32),
    );
    for (k, knob) in vj.dashboard.iter().enumerate() {
        push(
            format!("/composition/dashboard/link{}", k + 1),
            level(knob.value),
        );
    }
    for mapping in &vj.mappings {
        let Source::Osc { address } = &mapping.source else {
            continue;
        };
        if out.iter().any(|m| &m.address == address) {
            continue;
        }
        if let Some(value) = learned(vj, live, mapping) {
            out.push(Message {
                address: address.clone(),
                value,
            });
        }
    }
    out
}

/// What was sent to a destination, so only a change is sent again and no address more than every
/// [`MIN_INTERVAL`].
#[derive(Debug, Default)]
pub struct Throttle {
    /// By address: the value sent last and when.
    sent: HashMap<String, (Value, Duration)>,
}

impl Throttle {
    /// Forgets what was sent, so the next look sends everything (a destination that is new, or
    /// came back).
    pub fn forget(&mut self) {
        self.sent.clear();
    }

    /// The messages of `all` to send at `now` (any steady clock): the ones that changed since they
    /// were last sent and whose address was not written to less than [`MIN_INTERVAL`] ago. A change
    /// held back stays different from what was sent, so a later look sends it.
    pub fn pick(&mut self, now: Duration, all: Vec<Message>) -> Vec<Message> {
        all.into_iter()
            .filter(|message| {
                let due = match self.sent.get(&message.address) {
                    Some((value, at)) => {
                        value.differs(message.value) && now.saturating_sub(*at) >= MIN_INTERVAL
                    }
                    None => true,
                };
                if due {
                    self.sent
                        .insert(message.address.clone(), (message.value, now));
                }
                due
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vj_mapping::add_mapping;

    fn show() -> Vj {
        let mut n = 0;
        let mut vj = Vj::starting(|| {
            n += 1;
            format!("g{n}")
        });
        let column = vj.decks[0].columns[0].id.clone();
        let mut clip = crate::vj::new_clip("c1".into(), "v".into(), None);
        clip.column = Some(column);
        vj.layers[1].clips.push(clip);
        vj
    }

    fn live<'a>(playing: &'a dyn Fn(&str) -> bool) -> Live<'a> {
        Live {
            playing,
            column: None,
            crossfader: 0.5,
            bpm: 120.0,
            paused: false,
        }
    }

    fn find<'a>(all: &'a [Message], address: &str) -> Option<&'a Message> {
        all.iter().find(|m| m.address == address)
    }

    #[test]
    fn the_default_addresses_should_echo_the_clips_faders_crossfader_tempo_and_knobs() {
        let mut vj = show();
        vj.layers[1].master = 0.25;
        vj.layers[1].transition = 5.0;
        vj.dashboard.push(crate::vj_mapping::Knob {
            value: 0.75,
            ..Default::default()
        });
        let none = |_: &str| false;
        let all = messages(&vj, &live(&none));
        let value = |a: &str| find(&all, a).unwrap().value;
        // The layers count from the bottom; the clip is in the second layer, first column.
        assert_eq!(
            value("/composition/layers/2/clips/1/connect"),
            Value::Int(0)
        );
        assert_eq!(value("/composition/layers/2/master"), Value::Float(0.25));
        assert_eq!(
            value("/composition/layers/2/video/opacity"),
            Value::Float(1.0)
        );
        assert_eq!(
            value("/composition/layers/2/audio/volume"),
            Value::Float(1.0)
        );
        assert_eq!(
            value("/composition/layers/2/transition/duration"),
            Value::Float(0.5)
        );
        assert_eq!(value("/composition/crossfader/phase"), Value::Float(0.5));
        assert_eq!(
            value("/composition/tempocontroller/tempo"),
            Value::Float(120.0)
        );
        assert_eq!(value("/composition/dashboard/link1"), Value::Float(0.75));
        assert_eq!(value("/composition/columns/1/connect"), Value::Int(0));
        // A slot with no clip says nothing.
        assert!(find(&all, "/composition/layers/1/clips/1/connect").is_none());
    }

    #[test]
    fn the_layer_and_clip_dashboard_knobs_should_be_echoed_on_their_addresses() {
        let mut vj = show();
        vj.layers[0].dashboard.push(crate::vj_mapping::Knob {
            value: 0.25,
            ..Default::default()
        });
        vj.layers[1].clips[0]
            .dashboard
            .push(crate::vj_mapping::Knob {
                value: 0.6,
                ..Default::default()
            });
        let none = |_: &str| false;
        let all = messages(&vj, &live(&none));
        let value = |a: &str| find(&all, a).unwrap().value;
        assert_eq!(
            value("/composition/layers/1/dashboard/link1"),
            Value::Float(0.25)
        );
        // The clip is in the second layer, first column: its dashboard follows its connect.
        assert_eq!(
            value("/composition/layers/2/clips/1/dashboard/link1"),
            Value::Float(0.6)
        );
        assert!(find(&all, "/composition/layers/2/dashboard/link1").is_none());
    }

    #[test]
    fn a_playing_clip_and_the_launched_column_should_read_as_connected() {
        let vj = show();
        let column = vj.decks[0].columns[0].id.clone();
        let playing = |id: &str| id == "c1";
        let mut live = live(&playing);
        live.column = Some(&column);
        let all = messages(&vj, &live);
        let value = |a: &str| find(&all, a).unwrap().value;
        assert_eq!(
            value("/composition/layers/2/clips/1/connect"),
            Value::Int(1)
        );
        assert_eq!(value("/composition/columns/1/connect"), Value::Int(1));
        assert_eq!(value("/composition/columns/2/connect"), Value::Int(0));
    }

    #[test]
    fn a_learned_osc_address_should_echo_its_target_through_the_mapping_range() {
        let mut vj = show();
        let layer = vj.layers[0].id.clone();
        vj.layers[0].opacity = 0.25;
        let source = |a: &str| Source::Osc { address: a.into() };
        add_mapping(
            &mut vj,
            source("/mine/opacity"),
            Target::LayerOpacity {
                layer: layer.clone(),
            },
            "m1".into(),
        )
        .unwrap();
        vj.mappings[0].min = 1.0;
        vj.mappings[0].max = 0.0;
        add_mapping(
            &mut vj,
            source("/mine/bypass"),
            Target::LayerBypass { layer },
            "m2".into(),
        )
        .unwrap();
        vj.layers[0].bypass = true;
        add_mapping(
            &mut vj,
            Source::Midi {
                port: None,
                channel: 1,
                kind: crate::vj_mapping::MidiKind::Note,
                number: 1,
            },
            Target::Crossfader,
            "m3".into(),
        )
        .unwrap();
        let none = |_: &str| false;
        let all = messages(&vj, &live(&none));
        // Opacity 0.25 on a range of 1 down to 0 is the source at 0.75.
        assert_eq!(
            find(&all, "/mine/opacity").unwrap().value,
            Value::Float(0.75)
        );
        assert_eq!(find(&all, "/mine/bypass").unwrap().value, Value::Int(1));
        assert_eq!(
            all.iter()
                .filter(|m| m.address.starts_with("/mine"))
                .count(),
            2
        );
    }

    #[test]
    fn only_a_change_should_be_sent_and_never_faster_than_thirty_times_a_second() {
        let mut vj = show();
        let none = |_: &str| false;
        let mut throttle = Throttle::default();
        let ms = Duration::from_millis;
        let first = throttle.pick(ms(0), messages(&vj, &live(&none)));
        assert!(first.len() > 5, "the whole state goes first");
        // Nothing moved: nothing is sent, however long it takes.
        assert!(
            throttle
                .pick(ms(100), messages(&vj, &live(&none)))
                .is_empty()
        );
        // A fader moves, and is sent...
        vj.layers[0].master = 0.5;
        let sent = throttle.pick(ms(200), messages(&vj, &live(&none)));
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].address, "/composition/layers/1/master");
        // ...moves again 10 ms later: held back, then sent once the interval has gone.
        vj.layers[0].master = 0.6;
        assert!(
            throttle
                .pick(ms(210), messages(&vj, &live(&none)))
                .is_empty()
        );
        let late = throttle.pick(ms(240), messages(&vj, &live(&none)));
        assert_eq!(late.len(), 1);
        assert_eq!(late[0].value, Value::Float(0.6));
        // A sweep of 100 values in a second sends at most 30 of them.
        let mut count = 0;
        for step in 0..100_u64 {
            vj.layers[0].master = 0.6 + 0.004 * step as f32;
            count += throttle
                .pick(ms(1000 + step * 10), messages(&vj, &live(&none)))
                .len();
        }
        assert!(count <= 30, "{count}");
        assert!(count >= 25, "{count}");
        // A new destination gets everything again.
        throttle.forget();
        assert!(throttle.pick(ms(5000), messages(&vj, &live(&none))).len() > 5);
    }

    #[test]
    fn destinations_should_be_host_and_port_without_repeats() {
        assert_eq!(parse_destinations("").unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_destinations("10.0.0.2:9001, localhost:7000 10.0.0.2:9001").unwrap(),
            ["10.0.0.2:9001", "localhost:7000"]
        );
        assert_eq!(parse_destinations("[::1]:9000").unwrap(), ["[::1]:9000"]);
        assert_eq!(split_destination("[::1]:9000"), Some(("::1", 9000)));
        for bad in [
            "10.0.0.2",
            "host:0",
            "host:70000",
            ":9000",
            "a:1 b:2 c:3 d:4 e:5",
        ] {
            assert!(parse_destinations(bad).is_err(), "{bad}");
        }
    }
}
