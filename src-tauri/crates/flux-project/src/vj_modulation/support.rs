//! Test helpers shared by the tests of the modulation modules.

use super::*;
use crate::vj::Vj;
use crate::vj_effects::{Pace, Sync};

pub fn free(rate: f32) -> Pace {
    Pace {
        sync: Sync::Free,
        rate,
        beats: 1.0,
    }
}

pub fn beats(beats: f32) -> Pace {
    Pace {
        sync: Sync::Beats,
        rate: 0.0,
        beats,
    }
}

pub fn lfo(shape: Shape, pace: Pace, phase: f32, min: f32, max: f32) -> Modulation {
    Modulation {
        source: Source::Lfo { shape, pace, phase },
        min,
        max,
    }
}

pub fn audio(band: Band, gain: f32, attack: f32, release: f32) -> Modulation {
    Modulation {
        source: Source::Audio {
            band,
            gain,
            attack,
            release,
        },
        min: 0.0,
        max: 1.0,
    }
}

pub fn at(seconds: f64, beats: f64) -> Clock {
    Clock { seconds, beats }
}

pub fn grid_with_clip() -> Vj {
    let mut next = 0;
    let mut vj = Vj::starting(|| {
        next += 1;
        format!("g{next}")
    });
    let column = vj.decks[0].columns[0].id.clone();
    let mut clip = crate::vj::new_clip("c".into(), "v".into(), None);
    clip.column = Some(column);
    vj.layers[0].clips.push(clip);
    vj
}

pub fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-5, "{a} is not {b}");
}

pub fn text_clip_grid() -> Vj {
    let mut vj = grid_with_clip();
    vj.layers[0].clips[0].asset.clear();
    vj.layers[0].clips[0].generator =
        Some(serde_json::from_str(r#"{"kind":"text","text":"FLUX"}"#).unwrap());
    vj
}

pub fn text_number(vj: &Vj, name: &str) -> f32 {
    let json = serde_json::to_value(vj.layers[0].clips[0].generator.as_ref().unwrap()).unwrap();
    json[name].as_f64().unwrap() as f32
}

/// A grid whose knob 2 drives the opacity of layer 0 from 0.2 to 0.8, and is modulated by a
/// saw over `min..max`.
pub fn modulated_knob(min: f32, max: f32) -> Vj {
    let mut vj = grid_with_clip();
    let layer = vj.layers[0].id.clone();
    crate::vj_mapping::link_knob(
        &mut vj,
        2,
        crate::vj_mapping::Target::LayerOpacity { layer },
        Some(0.2),
        Some(0.8),
    )
    .unwrap();
    let m = lfo(Shape::SawUp, beats(1.0), 0.0, min, max);
    crate::vj_dashboard::modulate(&mut vj, &Default::default(), 2, Some(m)).unwrap();
    vj
}
