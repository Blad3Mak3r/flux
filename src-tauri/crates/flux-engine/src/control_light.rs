//! Light feedback: what a MIDI controller is sent so that its pads show the state of the show.
//!
//! Which pad is lit is derived from the same facts the Live view draws (the transport, the cues,
//! the show's tick), so a controller can never disagree with the screen. [`light`] decides it for
//! one action; [`Lamps`] remembers what each pad was last sent and writes only what changed. Both
//! are pure and tested with a recording [`MidiWrite`]. The thread that samples the facts and owns
//! the port is the app's (`src-tauri/src/control.rs`); nothing here runs in an audio callback or
//! logs.

use std::collections::HashMap;

use crate::control::{Binding, ControlAction, Feedback, MidiTrigger, TriggerKind, seek_target};
use crate::cues::Cue;
use crate::midi_cue::MidiWrite;
use crate::output::connect_midi;
use crate::show::{GoAction, ShowTick};
use crate::transport::TransportState;

const NOTE_ON: u8 = 0x90;
const CONTROL_CHANGE: u8 = 0xB0;

const CLIENT_NAME: &str = "FLUX control light";

/// How a pad should look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    Off,
    On,
    /// Alternates between lit and out; [`Lamps::update`] is told which half of the blink it is.
    Blink,
}

/// What the show is doing, as the Live view knows it.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// The transport of the running output, `None` with no output open.
    pub transport: Option<TransportState>,
    /// The position, with the timeline's start offset, and the cues the output plays.
    pub position: u64,
    pub cues: &'a [Cue],
    /// Where the active timeline begins and its nominal frame rate, as [`seek_target`] takes them.
    pub start: u64,
    pub fps: u8,
    /// The tick of a prepared setlist, `None` when the output plays a single timeline.
    pub show: Option<ShowTick>,
    /// The place of the active timeline among the project's, from 0, and how many there are.
    pub timeline: Option<(usize, usize)>,
}

impl Facts<'_> {
    fn open(&self) -> bool {
        self.transport.is_some()
    }

    fn is(&self, state: TransportState) -> Light {
        if self.transport == Some(state) {
            Light::On
        } else {
            Light::Off
        }
    }
}

fn lit(on: bool) -> Light {
    if on { Light::On } else { Light::Off }
}

/// How the pad of `action` looks, or `None` when it has no state to show (the view is the
/// window's, and the backend does not know it).
pub fn light(action: ControlAction, facts: &Facts) -> Option<Light> {
    let show = facts.show.unwrap_or_default();
    let can_change_timeline = facts.transport != Some(TransportState::Playing);
    let timeline = |available: fn(usize, usize) -> bool| {
        lit(can_change_timeline
            && facts
                .timeline
                .is_some_and(|(at, count)| available(at, count)))
    };
    Some(match action {
        ControlAction::Play => facts.is(TransportState::Playing),
        ControlAction::Toggle => facts.is(TransportState::Playing),
        ControlAction::Pause => facts.is(TransportState::Paused),
        ControlAction::Stop => facts.is(TransportState::Stopped),
        // GO waits for the operator, and blinks while a devamp is asked for.
        ControlAction::Go if show.devamp_pending => Light::Blink,
        ControlAction::Go => lit(facts.show.is_some() && show.go != GoAction::None),
        // Devamp is available inside a loop, and the turn counts from 1 while in one.
        ControlAction::Devamp if show.devamp_pending => Light::Blink,
        ControlAction::Devamp => lit(facts.show.is_some() && show.turn > 0),
        ControlAction::CancelDevamp => lit(show.devamp_pending),
        ControlAction::NextCue
        | ControlAction::PrevCue
        | ControlAction::GotoCue(_)
        | ControlAction::StepFrames(_)
        | ControlAction::StepSeconds(_) => lit(facts.open()
            && seek_target(action, facts.position, facts.cues, facts.start, facts.fps).is_some()),
        ControlAction::NextTimeline => timeline(|at, count| at + 1 < count),
        ControlAction::PrevTimeline => timeline(|at, _| at > 0),
        ControlAction::SelectTimeline(place) => {
            let place = place as usize;
            lit(can_change_timeline
                && facts
                    .timeline
                    .is_some_and(|(_, count)| (1..=count).contains(&place)))
        }
        ControlAction::ViewEdit | ControlAction::ViewLive => return None,
    })
}

/// The message that sets the pad of `trigger` to `value`. A program change has no "off", so it
/// is never lit.
fn message(trigger: MidiTrigger, value: u8) -> Option<[u8; 3]> {
    let status = match trigger.kind {
        TriggerKind::Note => NOTE_ON,
        TriggerKind::Cc => CONTROL_CHANGE,
        TriggerKind::Program => return None,
    };
    // A note on at velocity 0 is how a note off is sent.
    Some([
        status | (trigger.channel.saturating_sub(1) & 0x0F),
        trigger.number,
        value & 0x7F,
    ])
}

/// What each pad was last sent, so that only a change is written.
#[derive(Debug, Default)]
pub struct Lamps {
    /// The value on the pad and the one that puts it out, in case it is cleared later.
    sent: HashMap<MidiTrigger, (u8, u8)>,
}

impl Lamps {
    /// Writes to `port` the pads whose value is not the one they hold. The first call writes all
    /// of them, so the controller starts in step with the show. A pad whose write failed is
    /// written again by the next call. Returns how many writes failed, for the caller to count.
    ///
    /// `preset` is the feedback of the bindings that carry none; `blink_on` is which half of the
    /// blink it is.
    pub fn update(
        &mut self,
        bindings: &[Binding],
        preset: Feedback,
        facts: &Facts,
        blink_on: bool,
        port: &mut impl MidiWrite,
    ) -> usize {
        let mut failed = 0;
        for binding in bindings {
            let Some(light) = light(binding.action, facts) else {
                continue;
            };
            let feedback = binding.feedback.unwrap_or(preset);
            let value = match light {
                Light::On => feedback.on,
                Light::Blink if blink_on => feedback.on,
                Light::Off | Light::Blink => feedback.off,
            };
            if self.sent.get(&binding.trigger) == Some(&(value, feedback.off)) {
                continue;
            }
            let Some(bytes) = message(binding.trigger, value) else {
                continue;
            };
            if port.write(&bytes) {
                self.sent.insert(binding.trigger, (value, feedback.off));
            } else {
                failed += 1;
            }
        }
        failed
    }

    /// Puts out every pad that is lit and forgets them all, for when the feedback ends.
    pub fn clear(&mut self, port: &mut impl MidiWrite) {
        for (trigger, (value, off)) in self.sent.drain() {
            if value != off
                && let Some(bytes) = message(trigger, off)
            {
                port.write(&bytes);
            }
        }
    }

    /// Forgets what was sent, so that the next [`Self::update`] writes every pad again: the port
    /// was reopened and the controller may have lost its lights.
    pub fn forget(&mut self) {
        self.sent.clear();
    }
}

/// The MIDI output of a controller. Dropping it closes the port.
pub struct LightPort {
    name: String,
    connection: midir::MidiOutputConnection,
}

impl LightPort {
    /// Opens the output port a stored name hint refers to (`index` picks among ports of the same
    /// name). A controller has an input and an output port that carry the same name.
    pub fn open(hint: &str, index: usize) -> Result<Self, String> {
        let (_, name, connection) = connect_midi(hint, index, CLIENT_NAME)?;
        Ok(Self { name, connection })
    }

    /// The name of the port as the system lists it.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl MidiWrite for LightPort {
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.connection.write(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cues::CueList;
    use flux_project::{AudioAsset, AudioClip, FrameRate, Project, Track, TrackKind};

    /// A port that remembers what it was sent, and can be made to fail.
    #[derive(Default)]
    struct Recorder {
        sent: Vec<Vec<u8>>,
        fail: bool,
    }

    impl MidiWrite for Recorder {
        fn write(&mut self, bytes: &[u8]) -> bool {
            if self.fail {
                return false;
            }
            self.sent.push(bytes.to_vec());
            true
        }
    }

    impl Recorder {
        fn take(&mut self) -> Vec<Vec<u8>> {
            std::mem::take(&mut self.sent)
        }
    }

    fn pad(kind: TriggerKind, number: u8) -> MidiTrigger {
        MidiTrigger {
            kind,
            channel: 1,
            number,
        }
    }

    fn bind(number: u8, action: ControlAction) -> Binding {
        Binding {
            trigger: pad(TriggerKind::Note, number),
            action,
            feedback: None,
        }
    }

    /// Cues at frames 1000 and 1100 of a 25 fps timeline that starts at frame 1000.
    fn cue_list() -> CueList {
        let mut project = Project::new("Show", FrameRate::Fps25);
        project.timelines[0].timecode.start = 1000;
        project.assets.push(AudioAsset {
            id: "a1".into(),
            name: "Intro".into(),
            file: "audio/intro.wav".into(),
            bpm: None,
        });
        project.timelines[0].tracks.push(Track {
            audio_output: Default::default(),
            id: "t1".into(),
            name: "t1".into(),
            muted: false,
            gain_db: None,
            color: None,
            kind: TrackKind::Audio {
                clips: vec![
                    AudioClip::new("c1", "a1", 0),
                    AudioClip::new("c2", "a1", 100),
                ],
            },
        });
        CueList::build(&project, &project.timelines[0], |_| Some(1))
    }

    struct Scene {
        cues: CueList,
        transport: Option<TransportState>,
        position: u64,
        show: Option<ShowTick>,
        timeline: Option<(usize, usize)>,
    }

    impl Scene {
        fn new(transport: Option<TransportState>) -> Self {
            Self {
                cues: cue_list(),
                transport,
                position: 1000,
                show: None,
                timeline: Some((1, 3)),
            }
        }

        fn facts(&self) -> Facts<'_> {
            Facts {
                transport: self.transport,
                position: self.position,
                cues: self.cues.cues(),
                start: 1000,
                fps: 25,
                show: self.show,
                timeline: self.timeline,
            }
        }

        fn light(&self, action: ControlAction) -> Option<Light> {
            light(action, &self.facts())
        }

        fn update(&self, lamps: &mut Lamps, bindings: &[Binding], port: &mut Recorder) -> usize {
            lamps.update(bindings, Feedback::GENERIC, &self.facts(), true, port)
        }
    }

    fn tick(go: GoAction, devamp_pending: bool, turn: u32) -> ShowTick {
        ShowTick {
            go,
            devamp_pending,
            turn,
            ..ShowTick::default()
        }
    }

    #[test]
    fn the_transport_pads_should_follow_the_transport_state() {
        use ControlAction::{Pause, Play, Stop, Toggle};
        use Light::{Off, On};
        for (state, expected) in [
            (Some(TransportState::Playing), [On, Off, Off, On]),
            (Some(TransportState::Paused), [Off, On, Off, Off]),
            (Some(TransportState::Stopped), [Off, Off, On, Off]),
            (None, [Off, Off, Off, Off]),
        ] {
            let scene = Scene::new(state);
            let lights = [Play, Pause, Stop, Toggle].map(|action| scene.light(action));
            assert_eq!(lights, expected.map(Some), "{state:?}");
        }
    }

    #[test]
    fn go_should_be_lit_while_it_has_something_to_start_and_blink_while_a_devamp_waits() {
        let mut scene = Scene::new(Some(TransportState::Stopped));
        assert_eq!(
            scene.light(ControlAction::Go),
            Some(Light::Off),
            "no setlist"
        );

        for go in [GoAction::Start, GoAction::Resume, GoAction::Devamp] {
            scene.show = Some(tick(go, false, 0));
            assert_eq!(scene.light(ControlAction::Go), Some(Light::On), "{go:?}");
        }
        scene.show = Some(tick(GoAction::None, false, 0));
        assert_eq!(scene.light(ControlAction::Go), Some(Light::Off));

        scene.show = Some(tick(GoAction::Devamp, true, 2));
        assert_eq!(scene.light(ControlAction::Go), Some(Light::Blink));
    }

    #[test]
    fn devamp_and_cancel_should_follow_the_loop_and_the_request() {
        let mut scene = Scene::new(Some(TransportState::Playing));
        scene.show = Some(tick(GoAction::Devamp, false, 0));
        assert_eq!(scene.light(ControlAction::Devamp), Some(Light::Off));
        assert_eq!(scene.light(ControlAction::CancelDevamp), Some(Light::Off));

        scene.show = Some(tick(GoAction::Devamp, false, 1));
        assert_eq!(scene.light(ControlAction::Devamp), Some(Light::On));
        assert_eq!(scene.light(ControlAction::CancelDevamp), Some(Light::Off));

        scene.show = Some(tick(GoAction::Devamp, true, 1));
        assert_eq!(scene.light(ControlAction::Devamp), Some(Light::Blink));
        assert_eq!(scene.light(ControlAction::CancelDevamp), Some(Light::On));
    }

    #[test]
    fn the_cue_pads_should_be_lit_only_where_there_is_somewhere_to_go() {
        let mut scene = Scene::new(Some(TransportState::Stopped));
        scene.position = 1010;
        let lit = |scene: &Scene, action| scene.light(action) == Some(Light::On);
        assert!(lit(&scene, ControlAction::NextCue));
        assert!(!lit(&scene, ControlAction::PrevCue), "nothing before cue 1");
        assert!(lit(&scene, ControlAction::GotoCue(2)));
        assert!(!lit(&scene, ControlAction::GotoCue(3)), "no third cue");
        assert!(lit(&scene, ControlAction::StepSeconds(5)));

        scene.position = 1500;
        assert!(!lit(&scene, ControlAction::NextCue), "past the last cue");
        assert!(lit(&scene, ControlAction::PrevCue));

        scene.transport = None;
        for action in [
            ControlAction::NextCue,
            ControlAction::GotoCue(1),
            ControlAction::StepFrames(1),
        ] {
            assert_eq!(
                scene.light(action),
                Some(Light::Off),
                "no output: {action:?}"
            );
        }
    }

    #[test]
    fn the_timeline_pads_should_be_lit_when_it_exists_and_the_show_is_not_playing() {
        let mut scene = Scene::new(Some(TransportState::Stopped));
        scene.timeline = Some((0, 3));
        assert_eq!(scene.light(ControlAction::PrevTimeline), Some(Light::Off));
        assert_eq!(scene.light(ControlAction::NextTimeline), Some(Light::On));
        assert_eq!(
            scene.light(ControlAction::SelectTimeline(3)),
            Some(Light::On)
        );
        assert_eq!(
            scene.light(ControlAction::SelectTimeline(4)),
            Some(Light::Off)
        );

        scene.timeline = Some((2, 3));
        assert_eq!(scene.light(ControlAction::NextTimeline), Some(Light::Off));
        assert_eq!(scene.light(ControlAction::PrevTimeline), Some(Light::On));

        scene.transport = Some(TransportState::Playing);
        assert_eq!(scene.light(ControlAction::PrevTimeline), Some(Light::Off));
        scene.transport = None;
        scene.timeline = None;
        assert_eq!(scene.light(ControlAction::NextTimeline), Some(Light::Off));
    }

    #[test]
    fn the_view_pads_should_have_no_light() {
        let scene = Scene::new(None);
        assert_eq!(scene.light(ControlAction::ViewEdit), None);
        assert_eq!(scene.light(ControlAction::ViewLive), None);
    }

    #[test]
    fn the_first_update_should_write_every_learned_pad_and_nothing_else() {
        let scene = Scene::new(Some(TransportState::Playing));
        let bindings = [
            bind(36, ControlAction::Play),
            bind(37, ControlAction::Stop),
            bind(38, ControlAction::ViewLive),
        ];
        let (mut lamps, mut port) = (Lamps::default(), Recorder::default());

        let failed = scene.update(&mut lamps, &bindings, &mut port);

        assert_eq!(failed, 0);
        assert_eq!(
            port.take(),
            vec![vec![0x90, 36, 127], vec![0x90, 37, 0]],
            "play is lit, stop is out, the view pad and every unlearned pad are untouched"
        );
    }

    #[test]
    fn a_change_of_state_should_write_only_the_pads_it_changes() {
        let mut scene = Scene::new(Some(TransportState::Stopped));
        let bindings = [
            bind(36, ControlAction::Play),
            bind(37, ControlAction::Pause),
            bind(38, ControlAction::Stop),
        ];
        let (mut lamps, mut port) = (Lamps::default(), Recorder::default());
        scene.update(&mut lamps, &bindings, &mut port);
        port.take();

        scene.update(&mut lamps, &bindings, &mut port);
        assert_eq!(port.take(), Vec::<Vec<u8>>::new(), "nothing changed");

        scene.transport = Some(TransportState::Playing);
        scene.update(&mut lamps, &bindings, &mut port);
        assert_eq!(port.take(), vec![vec![0x90, 36, 127], vec![0x90, 38, 0]]);

        scene.transport = Some(TransportState::Paused);
        scene.update(&mut lamps, &bindings, &mut port);
        assert_eq!(port.take(), vec![vec![0x90, 36, 0], vec![0x90, 37, 127]]);
    }

    #[test]
    fn a_binding_should_use_its_own_values_the_channel_and_the_kind_of_its_trigger() {
        let scene = Scene::new(Some(TransportState::Playing));
        let cc = MidiTrigger {
            kind: TriggerKind::Cc,
            channel: 3,
            number: 20,
        };
        let bindings = [
            Binding {
                trigger: cc,
                action: ControlAction::Play,
                feedback: Some(Feedback { on: 21, off: 3 }),
            },
            bind_kind(TriggerKind::Program, 5),
            bind(40, ControlAction::Play),
        ];
        let (mut lamps, mut port) = (Lamps::default(), Recorder::default());

        lamps.update(
            &bindings,
            Feedback { on: 1, off: 0 },
            &scene.facts(),
            true,
            &mut port,
        );

        assert_eq!(
            port.take(),
            vec![vec![0xB2, 20, 21], vec![0x90, 40, 1]],
            "the control change keeps its own values, a program change is never lit, the note uses the preset"
        );
    }

    fn bind_kind(kind: TriggerKind, number: u8) -> Binding {
        Binding {
            trigger: pad(kind, number),
            action: ControlAction::Play,
            feedback: None,
        }
    }

    #[test]
    fn a_blinking_pad_should_alternate_with_the_blink_and_write_once_per_half() {
        let mut scene = Scene::new(Some(TransportState::Playing));
        scene.show = Some(tick(GoAction::Devamp, true, 1));
        let bindings = [bind(36, ControlAction::Go)];
        let (mut lamps, mut port) = (Lamps::default(), Recorder::default());

        for blink_on in [true, true, false, false, true] {
            lamps.update(
                &bindings,
                Feedback::GENERIC,
                &scene.facts(),
                blink_on,
                &mut port,
            );
        }

        assert_eq!(
            port.take(),
            vec![vec![0x90, 36, 127], vec![0x90, 36, 0], vec![0x90, 36, 127]]
        );
    }

    #[test]
    fn a_failed_write_should_be_counted_and_written_again_by_the_next_update() {
        let scene = Scene::new(Some(TransportState::Playing));
        let bindings = [bind(36, ControlAction::Play), bind(37, ControlAction::Stop)];
        let mut lamps = Lamps::default();
        let mut port = Recorder {
            fail: true,
            ..Recorder::default()
        };

        assert_eq!(scene.update(&mut lamps, &bindings, &mut port), 2);

        port.fail = false;
        assert_eq!(scene.update(&mut lamps, &bindings, &mut port), 0);
        assert_eq!(port.take(), vec![vec![0x90, 36, 127], vec![0x90, 37, 0]]);
    }

    #[test]
    fn clearing_should_put_out_only_the_lit_pads_and_a_forgotten_state_should_write_all_again() {
        let scene = Scene::new(Some(TransportState::Playing));
        let bindings = [bind(36, ControlAction::Play), bind(37, ControlAction::Stop)];
        let (mut lamps, mut port) = (Lamps::default(), Recorder::default());
        scene.update(&mut lamps, &bindings, &mut port);
        port.take();

        lamps.forget();
        scene.update(&mut lamps, &bindings, &mut port);
        assert_eq!(port.take().len(), 2, "both pads again after a reopen");

        lamps.clear(&mut port);
        assert_eq!(port.take(), vec![vec![0x90, 36, 0]]);
        lamps.clear(&mut port);
        assert_eq!(
            port.take(),
            Vec::<Vec<u8>>::new(),
            "nothing is left to put out"
        );
    }

    #[test]
    fn the_apc_family_should_light_green_and_any_other_controller_full_on() {
        assert_eq!(Feedback::preset("APC40 mkII"), Feedback { on: 1, off: 0 });
        assert_eq!(Feedback::preset("APC mini"), Feedback { on: 1, off: 0 });
        assert_eq!(Feedback::preset("Launchpad Mini"), Feedback::GENERIC);
        assert!(Feedback { on: 127, off: 0 }.is_valid());
        assert!(!Feedback { on: 128, off: 0 }.is_valid());
    }
}
