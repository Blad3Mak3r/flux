//! Fixtures shared by the tests of the model modules and of other modules.

use super::*;
use crate::events::{MidiEvent, MidiPortRef, OscDestination, OscEvent};
use crate::{MidiMessage, MscCommand, OscArg};
use flux_core::FrameRate;

pub fn sample_project() -> Project {
    Project {
        mode: Default::default(),
        vj: Default::default(),
        setlists: Vec::new(),
        voice: None,
        autosave: Autosave::default(),
        format_version: FORMAT_VERSION,
        name: "Opening night".into(),
        meta: Meta {
            id: "6f0c1d52-7a0e-4a52-9c3b-2d6a5f1b8e10".into(),
            created_at: "2026-11-01T09:30:00Z".into(),
            modified_at: "2026-11-05T14:29:03Z".into(),
            created_with: "0.1.0".into(),
            saved_with: "0.1.0".into(),
            author: "Ana".into(),
            description: String::new(),
        },
        assets: vec![AudioAsset {
            id: "a1".into(),
            name: "Click".into(),
            file: "audio/click.wav".into(),
            link: None,
            bpm: Some(128.5),
        }],
        videos: Vec::new(),
        timelines: vec![Timeline {
            controls: Vec::new(),
            id: "l1".into(),
            name: "Opening night".into(),
            timecode: TimecodeSettings {
                frame_rate: FrameRate::Fps2997Drop,
                start: 107_892,
            },
            tempo: Tempo {
                bpm: 128.0,
                beats_per_bar: 3,
                click: true,
            },
            tracks: vec![
                Track {
                    audio_output: Default::default(),
                    id: "t1".into(),
                    name: "Backing".into(),
                    muted: false,
                    gain_db: Some(-6.0),
                    color: Some("#5cd0ea".into()),
                    kind: TrackKind::Audio {
                        clips: vec![AudioClip {
                            gain_db: -3.5,
                            ..AudioClip::new("c1", "a1", 120)
                        }],
                    },
                },
                Track {
                    audio_output: Default::default(),
                    id: "t2".into(),
                    name: "Notes".into(),
                    muted: true,
                    gain_db: None,
                    color: None,
                    kind: TrackKind::Notes {
                        notes: vec![
                            Note {
                                id: "n1".into(),
                                at: 300,
                                length: None,
                                text: "Lights up".into(),
                            },
                            Note {
                                id: "n2".into(),
                                at: 450,
                                length: Some(75),
                                text: "Singer walks on\nstage left".into(),
                            },
                        ],
                    },
                },
                Track {
                    audio_output: Default::default(),
                    id: "t3".into(),
                    name: "Console".into(),
                    muted: false,
                    gain_db: None,
                    color: None,
                    kind: TrackKind::Midi {
                        port: Some(MidiPortRef {
                            port_name: "USB MIDI".into(),
                            port_index: 0,
                        }),
                        events: vec![
                            MidiEvent {
                                first_turn_only: false,
                                id: "m1".into(),
                                at: 600,
                                message: MidiMessage::NoteOn {
                                    channel: 1,
                                    note: 60,
                                    velocity: 100,
                                    length: Some(25),
                                },
                            },
                            MidiEvent {
                                first_turn_only: false,
                                id: "m2".into(),
                                at: 700,
                                message: MidiMessage::Msc {
                                    device_id: 127,
                                    format: 1,
                                    command: MscCommand::Go,
                                    cue: Some("12.5".into()),
                                    list: None,
                                },
                            },
                        ],
                    },
                },
                Track {
                    audio_output: Default::default(),
                    id: "t4".into(),
                    name: "QLab".into(),
                    muted: false,
                    gain_db: None,
                    color: None,
                    kind: TrackKind::Osc {
                        destination: Some("d1".into()),
                        events: vec![OscEvent {
                            first_turn_only: false,
                            id: "o1".into(),
                            at: 800,
                            address: "/cue/1/start".into(),
                            args: vec![OscArg::Int(1), OscArg::Float(0.5), OscArg::Bool(true)],
                        }],
                    },
                },
            ],
        }],
        output: OutputConfig {
            audio_routes: Vec::new(),
            track_routes: Vec::new(),
            operator: None,
            driver: Some("PipeWire".into()),
            audio: Some(AudioOutput {
                device_name: "Main Interface".into(),
                channels: vec![1, 2],
            }),
            ltc: vec![LtcOutput {
                device_name: "Main Interface".into(),
                channel: 3,
                level: Some(0.25),
                offset_ms: Some(-12),
            }],
            mtc: vec![MtcOutput {
                port_name: "IAC Driver Bus 1".into(),
                port_index: 1,
                offset_ms: Some(30),
            }],
            osc: vec![OscDestination {
                id: "d1".into(),
                name: "QLab".into(),
                host: "192.168.1.20".into(),
                port: 53000,
            }],
            preview: None,
            metronome: Some(MetronomeOutput {
                device_name: "Band Monitor".into(),
                channels: vec![3, 4],
                gain_db: -6.0,
            }),
        }
        .into(),
    }
}

/// The shape of the current format: what `sample_project` must serialise to and parse from.
pub const FIXTURE: &str = r##"{
    "formatVersion": 16,
    "autosave": { "enabled": true, "intervalSeconds": 30 },
    "name": "Opening night",
    "meta": {
        "id": "6f0c1d52-7a0e-4a52-9c3b-2d6a5f1b8e10",
        "createdAt": "2026-11-01T09:30:00Z",
        "modifiedAt": "2026-11-05T14:29:03Z",
        "createdWith": "0.1.0",
        "savedWith": "0.1.0",
        "author": "Ana"
    },
    "assets": [{ "id": "a1", "name": "Click", "file": "audio/click.wav", "bpm": 128.5 }],
    "timelines": [{
        "id": "l1", "name": "Opening night",
        "timecode": { "frameRate": "fps2997Drop", "start": 107892 },
        "tempo": { "bpm": 128.0, "beatsPerBar": 3, "click": true },
        "tracks": [
        {
            "id": "t1", "name": "Backing", "muted": false, "gainDb": -6.0, "color": "#5cd0ea",
            "kind": { "type": "audio", "clips": [
                { "id": "c1", "asset": "a1", "start": 120, "gainDb": -3.5 }
            ] }
        },
        {
            "id": "t2", "name": "Notes", "muted": true,
            "kind": { "type": "notes", "notes": [
                { "id": "n1", "at": 300, "text": "Lights up" },
                { "id": "n2", "at": 450, "length": 75, "text": "Singer walks on\nstage left" }
            ] }
        },
        {
            "id": "t3", "name": "Console", "muted": false,
            "kind": { "type": "midi", "port": { "portName": "USB MIDI" }, "events": [
                { "id": "m1", "at": 600, "message": { "type": "noteOn", "channel": 1, "note": 60, "velocity": 100, "length": 25 } },
                { "id": "m2", "at": 700, "message": { "type": "msc", "deviceId": 127, "format": 1, "command": "go", "cue": "12.5" } }
            ] }
        },
        {
            "id": "t4", "name": "QLab", "muted": false,
            "kind": { "type": "osc", "destination": "d1", "events": [
                { "id": "o1", "at": 800, "address": "/cue/1/start", "args": [
                    { "type": "int", "value": 1 }, { "type": "float", "value": 0.5 }, { "type": "bool", "value": true }
                ] }
            ] }
        }
    ] }],
    "output": {
        "metronome": { "gainDb": -6.0 },
        "ltc": [{ "slot": 1, "level": 0.25 }],
        "mtc": [{ "slot": 1 }],
        "osc": [{"id": "d1", "name": "QLab", "host": "192.168.1.20", "port": 53000}],
        "legacyRoutes": {
            "driver": "PipeWire",
            "audio": { "deviceName": "Main Interface", "channels": [1, 2] },
            "metronome": { "deviceName": "Band Monitor", "channels": [3, 4], "gainDb": -6.0 },
            "ltc": [{ "deviceName": "Main Interface", "channel": 3, "level": 0.25, "offsetMs": -12 }],
            "mtc": [{ "portName": "IAC Driver Bus 1", "portIndex": 1, "offsetMs": 30 }],
            "osc": [{ "id": "d1", "name": "QLab", "host": "192.168.1.20", "port": 53000 }]
        }
    }
}"##;

pub fn envelope_clip(points: &[(u64, f32)]) -> AudioClip {
    AudioClip {
        envelope: points
            .iter()
            .map(|&(at, gain_db)| EnvPoint {
                at,
                gain_db,
                curve: EnvCurve::Linear,
            })
            .collect(),
        ..AudioClip::new("c1", "a1", 0)
    }
}

pub fn project_with(clip: AudioClip) -> Project {
    let mut project = sample_project();
    let TrackKind::Audio { clips } = &mut project.timelines[0].tracks[0].kind else {
        panic!("the first track is audio");
    };
    clips[0] = AudioClip {
        id: clips[0].id.clone(),
        asset: clips[0].asset.clone(),
        ..clip
    };
    project
}

/// The sample project with a second timeline that holds one note.
pub fn two_timelines() -> Project {
    let mut project = sample_project();
    let mut second = Timeline::new("Second", FrameRate::Fps25);
    second.tracks.push(Track {
        audio_output: Default::default(),
        id: "t9".into(),
        name: "Notes".into(),
        muted: false,
        gain_db: None,
        color: None,
        kind: TrackKind::Notes {
            notes: vec![Note {
                id: "n3".into(),
                at: 0,
                length: None,
                text: String::new(),
            }],
        },
    });
    project.timelines.push(second);
    project
}

/// The sample project with a video track playing one bundled video.
pub fn video_project() -> Project {
    let mut project = sample_project();
    project.videos.push(VideoAsset {
        id: "v1".into(),
        name: "Intro".into(),
        file: "video/intro.mp4".into(),
        link: None,
    });
    project.timelines[0].tracks.push(Track {
        audio_output: Default::default(),
        id: "tv".into(),
        name: "Screen".into(),
        muted: false,
        gain_db: None,
        color: None,
        kind: TrackKind::Video {
            clips: vec![VideoClip {
                id: "cv".into(),
                asset: "v1".into(),
                start: 30,
                name: None,
                offset: 5,
                length: Some(100),
                fit: Default::default(),
                fade_in: 0,
                fade_out: 0,
            }],
        },
    });
    project
}

pub fn video_clip(project: &mut Project) -> &mut VideoClip {
    let track = project.timelines[0].tracks.last_mut().unwrap();
    let TrackKind::Video { clips } = &mut track.kind else {
        panic!("the last track is the video one");
    };
    &mut clips[0]
}
