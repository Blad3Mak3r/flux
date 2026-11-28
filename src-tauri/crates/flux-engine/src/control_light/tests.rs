use super::*;
use crate::cues::CueList;
use flux_project::vj_feedback::PadState;
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
        link: None,
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

#[test]
fn set_should_write_a_value_once_and_again_when_it_changes() {
    let mut lamps = Lamps::default();
    let mut port = Recorder::default();
    let pad = pad(TriggerKind::Note, 10);
    assert!(lamps.set(pad, 5, 0, &mut port));
    assert!(lamps.set(pad, 5, 0, &mut port));
    assert!(lamps.set(pad, 1, 0, &mut port));
    assert_eq!(port.take(), [vec![0x90, 10, 5], vec![0x90, 10, 1]]);
}

#[test]
fn set_should_send_a_control_change_on_the_channel_of_the_pad() {
    let mut lamps = Lamps::default();
    let mut port = Recorder::default();
    let cc = MidiTrigger {
        kind: TriggerKind::Cc,
        channel: 3,
        number: 7,
    };
    lamps.set(cc, 64, 0, &mut port);
    assert_eq!(port.take(), [vec![0xB2, 7, 64]]);
}

#[test]
fn set_should_try_again_after_a_failed_write() {
    let mut lamps = Lamps::default();
    let mut port = Recorder {
        fail: true,
        ..Recorder::default()
    };
    let pad = pad(TriggerKind::Note, 10);
    assert!(!lamps.set(pad, 5, 0, &mut port));
    port.fail = false;
    assert!(lamps.set(pad, 5, 0, &mut port));
    assert_eq!(port.take(), [vec![0x90, 10, 5]]);
}

#[test]
fn retain_should_put_out_only_the_lit_pads_that_were_dropped() {
    let mut lamps = Lamps::default();
    let mut port = Recorder::default();
    let (kept, lit, dark) = (
        pad(TriggerKind::Note, 1),
        pad(TriggerKind::Note, 2),
        pad(TriggerKind::Note, 3),
    );
    lamps.set(kept, 1, 0, &mut port);
    lamps.set(lit, 5, 0, &mut port);
    lamps.set(dark, 0, 0, &mut port);
    port.take();
    lamps.retain(&std::iter::once(kept).collect(), &mut port);
    assert_eq!(port.take(), [vec![0x90, 2, 0]]);
    lamps.clear(&mut port);
    assert_eq!(
        port.take(),
        [vec![0x90, 1, 0]],
        "the kept pad is still known"
    );
}

fn vj_pad(number: u8, state: PadState, lights: Option<Lights>) -> Pad {
    Pad {
        port: Some("APC40 mkII".into()),
        channel: 1,
        kind: MidiKind::Note,
        number,
        state,
        lights,
    }
}

#[test]
fn vj_pads_should_be_written_once_and_again_only_when_their_state_changes() {
    let (mut lamps, mut port) = (Lamps::default(), Recorder::default());
    let apc = Lights::APC;
    let mut pads = [
        vj_pad(1, PadState::Empty, None),
        vj_pad(2, PadState::Loaded, None),
        vj_pad(3, PadState::Playing, None),
    ];
    fn refs(pads: &[Pad]) -> Vec<&Pad> {
        pads.iter().collect()
    }
    lamps.update_pads(&refs(&pads), apc, &mut port);
    assert_eq!(
        port.take(),
        [vec![0x90, 1, 0], vec![0x90, 2, 5], vec![0x90, 3, 1]],
        "off, loaded yellow, playing green"
    );
    lamps.update_pads(&refs(&pads), apc, &mut port);
    assert!(port.take().is_empty(), "nothing changed, nothing sent");
    pads[1].state = PadState::Playing;
    pads[2].state = PadState::Loaded;
    lamps.update_pads(&refs(&pads), apc, &mut port);
    assert_eq!(port.take(), [vec![0x90, 2, 1], vec![0x90, 3, 5]]);
}

#[test]
fn a_vj_pad_should_use_its_own_colours_and_a_removed_mapping_should_go_out() {
    let (mut lamps, mut port) = (Lamps::default(), Recorder::default());
    let red = Lights {
        off: 0,
        loaded: 4,
        on: 3,
    };
    let pads = [
        vj_pad(1, PadState::Playing, Some(red)),
        vj_pad(2, PadState::On, None),
    ];
    lamps.update_pads(&pads.iter().collect::<Vec<_>>(), Lights::APC, &mut port);
    assert_eq!(port.take(), [vec![0x90, 1, 3], vec![0x90, 2, 1]]);
    lamps.update_pads(&[&pads[0]], Lights::APC, &mut port);
    assert_eq!(port.take(), [vec![0x90, 2, 0]]);
}

#[test]
fn closing_should_put_every_lit_vj_pad_out() {
    let (mut lamps, mut port) = (Lamps::default(), Recorder::default());
    let pads = [
        vj_pad(1, PadState::Playing, None),
        vj_pad(2, PadState::Loaded, None),
        vj_pad(3, PadState::Empty, None),
    ];
    lamps.update_pads(&pads.iter().collect::<Vec<_>>(), Lights::APC, &mut port);
    port.take();
    lamps.clear(&mut port);
    let mut out = port.take();
    out.sort();
    assert_eq!(out, [vec![0x90, 1, 0], vec![0x90, 2, 0]]);
}

#[test]
fn reopening_a_port_should_send_the_whole_state_again() {
    let (mut lamps, mut port) = (Lamps::default(), Recorder::default());
    let pads = [vj_pad(1, PadState::Playing, None)];
    let refs: Vec<_> = pads.iter().collect();
    lamps.update_pads(&refs, Lights::APC, &mut port);
    port.take();
    lamps.forget();
    lamps.update_pads(&refs, Lights::APC, &mut port);
    assert_eq!(port.take(), [vec![0x90, 1, 1]]);
}

#[test]
fn pads_should_be_grouped_by_the_port_they_were_learned_on() {
    let mut other = vj_pad(2, PadState::Off, None);
    other.port = Some("Launchpad".into());
    let mut none = vj_pad(3, PadState::Off, None);
    none.port = None;
    let pads = [vj_pad(1, PadState::Off, None), other, none];
    let ports = by_port(&pads, "APC40 mkII");
    assert_eq!(
        ports.keys().collect::<Vec<_>>(),
        ["APC40 mkII", "Launchpad"]
    );
    assert_eq!(
        ports["APC40 mkII"].len(),
        2,
        "a pad with no port uses the default"
    );
}

#[test]
fn a_control_change_pad_should_be_sent_as_a_control_change() {
    let (mut lamps, mut port) = (Lamps::default(), Recorder::default());
    let mut pad = vj_pad(7, PadState::On, None);
    pad.kind = MidiKind::Cc;
    pad.channel = 2;
    lamps.update_pads(&[&pad], Lights::GENERIC, &mut port);
    assert_eq!(port.take(), [vec![0xB1, 7, 127]]);
}
