//! Test helpers shared by the tests of the map file modules.

use super::*;
use crate::vj::{EffectTarget, Vj, VjOp};
use crate::vj_mapping::{MidiKind, Target};
use crate::{Project, ProjectMode};
use flux_core::FrameRate;

/// A VJ show whose ids all start with `prefix`, so two shows never share one.
pub fn show(prefix: &str) -> Project {
    let mut next = 0;
    let mut ids = move || {
        next += 1;
        format!("{prefix}{next}")
    };
    let mut project = Project::new("Show", FrameRate::Fps25);
    project.mode = ProjectMode::Vj;
    project.vj = Vj::starting(&mut ids);
    project
}

pub fn apply(project: &mut Project, op: VjOp) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    crate::vj::apply(project, op, || {
        format!("gen{}", NEXT.fetch_add(1, Ordering::Relaxed))
    })
    .unwrap();
}

pub fn cc(number: u8) -> Source {
    Source::Midi {
        port: Some("APC40".into()),
        channel: 1,
        kind: MidiKind::Cc,
        number,
    }
}

pub fn wipe() -> ImportChoice {
    ImportChoice {
        wipe: true,
        keep: Vec::new(),
    }
}

pub fn ids() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("new{n}")
    }
}

/// A show with a blur on its second layer, mapped from every kind of source.
pub fn mapped() -> Project {
    let mut project = show("a");
    let layer = project.vj.layers[1].id.clone();
    apply(
        &mut project,
        VjOp::AddEffect {
            target: EffectTarget::Layer(layer.clone()),
            kind: "blur".into(),
        },
    );
    let effect = project.vj.layers[1].effects[0].id.clone();
    let column = project.vj.decks[0].columns[2].id.clone();
    let sources_and_targets = [
        (
            cc(1),
            Target::LayerOpacity {
                layer: layer.clone(),
            },
        ),
        (
            Source::Artnet {
                universe: 1,
                channel: 37,
                fine: Some(38),
            },
            Target::EffectParam {
                effect,
                param: "radius".into(),
            },
        ),
        (
            Source::Key {
                code: "KeyQ".into(),
                ctrl: false,
                alt: false,
                shift: true,
                meta: false,
            },
            Target::ColumnLaunch { column },
        ),
        (
            Source::Osc {
                address: "/go".into(),
            },
            Target::LayerBypass { layer },
        ),
        (cc(2), Target::Crossfader),
    ];
    for (source, target) in sources_and_targets {
        apply(&mut project, VjOp::AddMapping { source, target });
    }
    let id = project.vj.mappings[0].id.clone();
    apply(
        &mut project,
        VjOp::SetMapping {
            mapping: id,
            min: 0.2,
            max: 0.9,
            invert: true,
            mode: ButtonMode::Trigger,
        },
    );
    let target = Target::LayerMaster {
        layer: project.vj.layers[2].id.clone(),
    };
    apply(
        &mut project,
        VjOp::DashboardLink {
            scope: Default::default(),
            knob: 3,
            target,
            min: Some(0.1),
            max: Some(0.8),
        },
    );
    apply(
        &mut project,
        VjOp::RenameKnob {
            scope: Default::default(),
            knob: 3,
            name: "Fade".into(),
        },
    );
    project
}

/// A show with the two mappings of `cc` 1 and 2 on its layers 0 and 1.
pub fn existing() -> Project {
    let mut into = show("b");
    for (number, layer) in [(1, 0), (2, 1)] {
        let layer = into.vj.layers[layer].id.clone();
        apply(
            &mut into,
            VjOp::AddMapping {
                source: cc(number),
                target: Target::LayerMaster { layer },
            },
        );
    }
    into
}

pub fn file() -> MapFile {
    let mut from = show("a");
    let layer = from.vj.layers[2].id.clone();
    // Overwrites the source of the show's first mapping, and the target of its second.
    apply(
        &mut from,
        VjOp::AddMapping {
            source: cc(1),
            target: Target::LayerOpacity { layer },
        },
    );
    let layer = from.vj.layers[1].id.clone();
    apply(
        &mut from,
        VjOp::AddMapping {
            source: cc(7),
            target: Target::LayerMaster { layer },
        },
    );
    let layer = from.vj.layers[0].id.clone();
    apply(
        &mut from,
        VjOp::AddMapping {
            source: cc(8),
            target: Target::LayerSolo { layer },
        },
    );
    parse(&export(&from.vj).unwrap()).unwrap()
}

pub fn sources(project: &Project) -> Vec<Source> {
    let mut found: Vec<_> = project
        .vj
        .mappings
        .iter()
        .map(|m| m.source.clone())
        .collect();
    found.sort_by_key(|source| format!("{source:?}"));
    found
}
