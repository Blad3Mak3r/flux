use super::*;
use crate::vj::{Autopilot, Layer, VjOp};
use crate::{Project, ProjectMode};
use flux_core::FrameRate;

mod edit;
mod follow;
mod osc;
mod react;
mod sources;

fn show() -> Project {
    let mut ids = (0..).map(|n| format!("id{n}"));
    let mut project = Project::new("Show", FrameRate::Fps25);
    project.mode = ProjectMode::Vj;
    project.vj = Vj::starting(|| ids.next().unwrap());
    project
}

fn apply(project: &mut Project, op: VjOp) -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(1000);
    crate::vj::apply(project, op, || {
        format!("new{}", NEXT.fetch_add(1, Ordering::Relaxed))
    })
}

fn cc(number: u8) -> Source {
    Source::Midi {
        port: Some("APC40".into()),
        channel: 1,
        kind: MidiKind::Cc,
        number,
    }
}

fn note(number: u8) -> Source {
    Source::Midi {
        port: None,
        channel: 1,
        kind: MidiKind::Note,
        number,
    }
}

fn heard(source: Source, level: f32, down: bool) -> Heard {
    Heard {
        source,
        level,
        down,
    }
}

fn layer(project: &Project, at: usize) -> &Layer {
    &project.vj.layers[at]
}

fn with_generator(project: &mut Project) -> String {
    let layer = layer(project, 0).id.clone();
    let column = project.vj.decks[0].columns[0].id.clone();
    apply(
        project,
        VjOp::PlaceGenerator {
            generator: crate::generator::Generator::Strobe {
                colour: "#ffffff".into(),
                rate: 8.0,
                width: 0.5,
            },
            layer,
            column,
        },
    )
    .unwrap();
    project.vj.layers[0].clips[0].id.clone()
}

fn key(code: &str, shift: bool) -> Source {
    Source::Key {
        code: code.into(),
        ctrl: false,
        alt: false,
        shift,
        meta: false,
    }
}

fn artnet(universe: u16, channel: u16, fine: Option<u16>) -> Source {
    Source::Artnet {
        universe,
        channel,
        fine,
    }
}
