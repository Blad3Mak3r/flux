//! LED feedback of the VJ mappings, as Resolume sends it: the pad a clip is mapped to lights up
//! with the state of the clip, and a mapped switch shows whether it is on.
//!
//! Everything here is pure. [`pads`] reads the grid (and which clips play, which the app knows) and
//! says what each mapped MIDI pad should look like; [`Pad::value`] turns that into the velocity or
//! control value the controller wants. The app (`src-tauri/src/control.rs`) owns the output port,
//! sends only what changed since the last look and puts every pad out on close.
//!
//! Which velocity means which colour is the controller's: [`Lights::preset`] knows the AKAI APC
//! family (APC40, APC mini), whose velocity picks a colour (the table is in `docs/vj-mode.md`);
//! any other controller gets a generic dim level and full on, which the user can change.

use serde::{Deserialize, Serialize};

use crate::vj::{Layer, Vj};
use crate::vj_mapping::{MidiKind, Source, Target};
use crate::{ProjectError, Result};

/// What a pad is sent for each of its states: a velocity (note) or a value (control change), 0 to
/// 127.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lights {
    /// Nothing in the slot, or a switch that is off.
    pub off: u8,
    /// A clip is there but not playing.
    pub loaded: u8,
    /// The clip plays, or the switch is on.
    pub on: u8,
}

impl Lights {
    pub const GENERIC: Self = Self {
        off: 0,
        loaded: 32,
        on: 127,
    };

    /// APC40 and APC mini velocities: 0 off, 1 green, 2 blinking green, 3 red, 4 blinking red,
    /// 5 yellow, 6 blinking yellow. Loaded is yellow and playing green, as in Resolume.
    pub const APC: Self = Self {
        off: 0,
        loaded: 5,
        on: 1,
    };

    /// The values to start from for the controller whose port is named `port`.
    pub fn preset(port: &str) -> Self {
        if Self::palette(port).is_some() {
            Self::APC
        } else {
            Self::GENERIC
        }
    }

    /// The name of the colour table the controller on `port` has, when its velocities pick a
    /// colour (`apc`: the values 0 to 6 above); `None` when a value is only a brightness.
    pub fn palette(port: &str) -> Option<&'static str> {
        port.to_ascii_lowercase().contains("apc").then_some("apc")
    }

    /// Whether every value fits a MIDI data byte.
    pub fn is_valid(self) -> bool {
        [self.off, self.loaded, self.on].iter().all(|v| *v < 0x80)
    }

    pub fn validate(self) -> Result<()> {
        if self.is_valid() {
            Ok(())
        } else {
            Err(ProjectError::Invalid(
                "a light is a MIDI value of 0 to 127".into(),
            ))
        }
    }
}

/// How a pad looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadState {
    /// Nothing to launch: a slot without a clip.
    Empty,
    /// A clip is in the slot.
    Loaded,
    Playing,
    /// A switch, off.
    Off,
    /// A switch, on.
    On,
}

/// A mapped pad and what it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pad {
    /// The input port the control was learned on, which the output port is found by; `None` is
    /// the controller of the control preferences.
    pub port: Option<String>,
    /// From 1 to 16.
    pub channel: u8,
    pub kind: MidiKind,
    pub number: u8,
    pub state: PadState,
    /// The mapping's own colours, over the grid's and the controller's.
    pub lights: Option<Lights>,
}

impl Pad {
    /// The value to send the pad, given the colours to use when it has none of its own.
    pub fn value(&self, base: Lights) -> u8 {
        let lights = self.lights.unwrap_or(base);
        match self.state {
            PadState::Empty | PadState::Off => lights.off,
            PadState::Loaded => lights.loaded,
            PadState::Playing | PadState::On => lights.on,
        }
    }

    /// The value that puts the pad out.
    pub fn off(&self, base: Lights) -> u8 {
        self.lights.unwrap_or(base).off
    }
}

/// The state of what `target` is, `None` for a target that shows nothing (a fader, a tap).
/// `playing` says whether a clip plays; `paused` whether the beats are held.
fn state(
    vj: &Vj,
    target: &Target,
    playing: &dyn Fn(&str) -> bool,
    paused: bool,
) -> Option<PadState> {
    let on = |on: bool| Some(if on { PadState::On } else { PadState::Off });
    let layer_plays = |layer: &Layer| layer.clips.iter().any(|c| playing(&c.id));
    let layer = |id: &str| vj.layers.iter().find(|l| l.id == id);
    match target {
        Target::ClipLaunch { clip } => Some(match vj.find_clip(clip) {
            Some((_, clip)) if playing(&clip.id) => PadState::Playing,
            Some(_) => PadState::Loaded,
            None => PadState::Empty,
        }),
        Target::ColumnLaunch { column } => {
            let mut clips = vj
                .column_clips(column)
                .filter_map(|(_, clip)| clip)
                .peekable();
            Some(if clips.peek().is_none() {
                PadState::Empty
            } else if clips.any(|c| playing(&c.id)) {
                PadState::Playing
            } else {
                PadState::Loaded
            })
        }
        Target::LayerStop { layer: id } => on(layer(id).is_some_and(layer_plays)),
        Target::StopAll => on(vj.layers.iter().any(layer_plays)),
        Target::DeckSelect { deck } => on(&vj.deck == deck),
        Target::LayerBypass { layer: id } => on(layer(id).is_some_and(|l| l.bypass)),
        Target::LayerSolo { layer: id } => on(layer(id).is_some_and(|l| l.solo)),
        Target::GroupBypass { group } => on(vj.group(group).is_some_and(|g| g.bypass)),
        Target::GroupSolo { group } => on(vj.group(group).is_some_and(|g| g.solo)),
        Target::EffectBypass { effect } => on(vj.find_effect(effect).is_some_and(|e| e.bypass)),
        Target::BpmPause => on(paused),
        _ => None,
    }
}

/// Every mapped MIDI pad of `vj` with what it should look like now. A pad is a note or a control
/// change; a pitch bend has no light.
pub fn pads(vj: &Vj, playing: &dyn Fn(&str) -> bool, paused: bool) -> Vec<Pad> {
    vj.mappings
        .iter()
        .filter_map(|m| {
            let Source::Midi {
                port,
                channel,
                kind,
                number,
            } = &m.source
            else {
                return None;
            };
            if *kind == MidiKind::PitchBend {
                return None;
            }
            // A parked mapping keeps its pad, dark: its slot is empty and its target is stale.
            let state = if m.parked {
                state(vj, &m.target, &|_| false, false).map(|_| PadState::Empty)?
            } else {
                state(vj, &m.target, playing, paused)?
            };
            Some(Pad {
                port: port.clone(),
                channel: *channel,
                kind: *kind,
                number: *number,
                state,
                lights: m.lights,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vj_mapping::Mapping;

    fn midi(number: u8) -> Source {
        Source::Midi {
            port: Some("APC40 mkII".into()),
            channel: 1,
            kind: MidiKind::Note,
            number,
        }
    }

    /// A grid with one clip, mapped to note 10, its layer's bypass to note 11 and the
    /// crossfader, which has no light, to note 12.
    fn grid() -> (Vj, String) {
        let mut n = 0;
        let mut vj = Vj::starting(|| {
            n += 1;
            format!("g{n}")
        });
        let column = vj.decks[0].columns[0].id.clone();
        let mut clip = crate::vj::new_clip("c1".into(), "v".into(), None);
        clip.column = Some(column);
        vj.layers[0].clips.push(clip);
        let layer = vj.layers[0].id.clone();
        let mappings = [
            (midi(10), Target::ClipLaunch { clip: "c1".into() }),
            (
                midi(11),
                Target::LayerBypass {
                    layer: layer.clone(),
                },
            ),
            (midi(12), Target::Crossfader),
        ];
        for (i, (source, target)) in mappings.into_iter().enumerate() {
            crate::vj_mapping::add_mapping(&mut vj, source, target, format!("m{i}")).unwrap();
        }
        (vj, layer)
    }

    fn states(vj: &Vj, playing: &[&str], paused: bool) -> Vec<(u8, PadState)> {
        pads(vj, &|id| playing.contains(&id), paused)
            .iter()
            .map(|p| (p.number, p.state))
            .collect()
    }

    #[test]
    fn a_clip_pad_should_be_loaded_then_playing_and_a_fader_should_have_no_light() {
        let (vj, _) = grid();
        assert_eq!(
            states(&vj, &[], false),
            [(10, PadState::Loaded), (11, PadState::Off)]
        );
        assert_eq!(
            states(&vj, &["c1"], false),
            [(10, PadState::Playing), (11, PadState::Off)]
        );
    }

    #[test]
    fn a_switch_pad_should_echo_the_switch() {
        let (mut vj, layer) = grid();
        vj.layers.iter_mut().find(|l| l.id == layer).unwrap().bypass = true;
        assert_eq!(states(&vj, &[], false)[1], (11, PadState::On));
    }

    #[test]
    fn a_pad_of_an_emptied_slot_should_be_dark() {
        let (mut vj, _) = grid();
        vj.mappings[0].parked = true;
        assert_eq!(states(&vj, &["c1"], false)[0], (10, PadState::Empty));
    }

    #[test]
    fn the_pause_pad_should_follow_the_beat_hold() {
        let (mut vj, _) = grid();
        crate::vj_mapping::add_mapping(&mut vj, midi(13), Target::BpmPause, "p".into()).unwrap();
        assert_eq!(states(&vj, &[], true).last(), Some(&(13, PadState::On)));
        assert_eq!(states(&vj, &[], false).last(), Some(&(13, PadState::Off)));
    }

    #[test]
    fn the_velocity_of_each_state_should_come_from_the_apc_table_or_the_mapping() {
        let apc = Lights::preset("Akai APC40 mkII 24:0");
        let pad = |state, lights| Pad {
            port: None,
            channel: 1,
            kind: MidiKind::Note,
            number: 0,
            state,
            lights,
        };
        assert_eq!(pad(PadState::Empty, None).value(apc), 0);
        assert_eq!(pad(PadState::Loaded, None).value(apc), 5, "yellow");
        assert_eq!(pad(PadState::Playing, None).value(apc), 1, "green");
        assert_eq!(pad(PadState::On, None).value(apc), 1);
        let red = Lights {
            off: 0,
            loaded: 4,
            on: 3,
        };
        assert_eq!(pad(PadState::Playing, Some(red)).value(apc), 3, "red");
        assert_eq!(Lights::preset("Launchpad"), Lights::GENERIC);
    }

    #[test]
    fn colours_should_be_stored_with_a_mapping_and_refused_past_a_data_byte() {
        let (mut vj, _) = grid();
        let id = vj.mappings[0].id.clone();
        let red = Lights {
            off: 0,
            loaded: 4,
            on: 3,
        };
        crate::vj_mapping::set_mapping_lights(&mut vj, &id, Some(red)).unwrap();
        assert_eq!(vj.mappings[0].lights, Some(red));
        let json = serde_json::to_string(&vj.mappings[0]).unwrap();
        let back: Mapping = serde_json::from_str(&json).unwrap();
        assert_eq!(back.lights, Some(red));
        let bad = Lights {
            off: 0,
            loaded: 128,
            on: 1,
        };
        assert!(crate::vj_mapping::set_mapping_lights(&mut vj, &id, Some(bad)).is_err());
    }
}
