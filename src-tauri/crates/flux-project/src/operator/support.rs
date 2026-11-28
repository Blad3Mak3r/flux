//! Test helpers shared by the tests of the operator modules.

use super::*;

pub fn order() -> VoiceOrder {
    VoiceOrder {
        mode: Default::default(),
        synthesis_text: None,
        trim_start: 0,
        trim_end: 0,
        id: "order".into(),
        at: 100,
        dialogue: "Stand by lights".into(),
        alignment: Alignment::Start,
        generated: Some(GeneratedVoice {
            origin: crate::operator::RecordingOrigin::Generated,
            synthesis_text: None,
            asset: "audio".into(),
            dialogue: "Stand by lights".into(),
            voice: "voice".into(),
            model: VOICE_MODEL.into(),
            samples: 48_001,
        }),
    }
}
