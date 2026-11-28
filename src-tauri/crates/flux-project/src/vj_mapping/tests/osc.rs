use super::*;

fn osc(project: &Project, address: &str, args: &[OscArg]) -> Option<(Target, Command)> {
    osc_command(&project.vj, address, args)
}

#[test]
fn the_fixed_addresses_should_count_from_one_by_position() {
    let mut project = show();
    let clip = with_generator(&mut project);
    let layers: Vec<String> = project.vj.layers.iter().map(|l| l.id.clone()).collect();
    let columns: Vec<String> = project.vj.decks[0]
        .columns
        .iter()
        .map(|c| c.id.clone())
        .collect();
    let one = [OscArg::Int(1)];
    let half = [OscArg::Float(0.5)];

    assert_eq!(
        osc(&project, "/composition/layers/1/clips/1/connect", &one),
        Some((Target::ClipLaunch { clip: clip.clone() }, Command::Press))
    );
    assert_eq!(
        osc(
            &project,
            "/composition/layers/1/clips/1/connect",
            &[OscArg::Int(0)]
        ),
        Some((Target::ClipLaunch { clip }, Command::Release))
    );
    assert_eq!(
        osc(&project, "/composition/layers/1/clips/2/connect", &one),
        None,
        "an empty cell"
    );
    assert_eq!(
        osc(&project, "/composition/columns/3/connect", &[]),
        Some((
            Target::ColumnLaunch {
                column: columns[2].clone()
            },
            Command::Press
        )),
        "no value is a press"
    );
    assert_eq!(
        osc(&project, "/composition/layers/2/video/opacity", &half),
        Some((
            Target::LayerOpacity {
                layer: layers[1].clone()
            },
            Command::Value(0.5)
        ))
    );
    assert_eq!(
        osc(
            &project,
            "/composition/layers/3/master",
            &[OscArg::Float(2.0)]
        ),
        Some((
            Target::LayerMaster {
                layer: layers[2].clone()
            },
            Command::Value(1.0)
        )),
        "held inside 0 to 1"
    );
    assert_eq!(
        osc(&project, "/composition/layers/1/audio/volume", &half),
        Some((
            Target::LayerVolume {
                layer: layers[0].clone()
            },
            Command::Value(0.5)
        ))
    );
    assert_eq!(
        osc(&project, "/composition/layers/1/transition/duration", &half),
        Some((
            Target::LayerTransition {
                layer: layers[0].clone()
            },
            Command::Value(5.0)
        ))
    );
    assert_eq!(
        osc(&project, "/composition/layers/1/bypassed", &one),
        Some((
            Target::LayerBypass {
                layer: layers[0].clone()
            },
            Command::Switch(true)
        ))
    );
    assert_eq!(
        osc(
            &project,
            "/composition/layers/1/solo",
            &[OscArg::Bool(false)]
        ),
        Some((
            Target::LayerSolo {
                layer: layers[0].clone()
            },
            Command::Switch(false)
        ))
    );
    assert_eq!(
        osc(&project, "/composition/layers/2/clear", &one),
        Some((
            Target::LayerStop {
                layer: layers[1].clone()
            },
            Command::Press
        ))
    );
    assert_eq!(
        osc(
            &project,
            "/composition/crossfader/phase",
            &[OscArg::Float(0.25)]
        ),
        Some((Target::Crossfader, Command::Value(0.25)))
    );
    assert_eq!(
        osc(&project, "/composition/layers/9/master", &half),
        None,
        "no ninth layer"
    );
}

#[test]
fn the_tempo_deck_and_dashboard_addresses_should_resolve() {
    let mut project = show();
    apply(&mut project, VjOp::AddDeck).unwrap();
    let decks: Vec<String> = project.vj.decks.iter().map(|d| d.id.clone()).collect();
    let one = [OscArg::Int(1)];

    assert_eq!(
        osc(
            &project,
            "/composition/tempocontroller/tempo",
            &[OscArg::Float(128.0)]
        ),
        Some((Target::Bpm, Command::Value(128.0)))
    );
    assert_eq!(
        osc(
            &project,
            "/composition/tempocontroller/tempo",
            &[OscArg::String("500".into())]
        ),
        Some((Target::Bpm, Command::Value(400.0))),
        "a string with a number, held inside the tempo range"
    );
    for (address, target) in [
        ("tempotap", Target::BpmTap),
        ("resync", Target::BpmResync),
        ("plus", Target::BpmPlus),
        ("minus", Target::BpmMinus),
        ("double", Target::BpmDouble),
        ("half", Target::BpmHalf),
    ] {
        assert_eq!(
            osc(
                &project,
                &format!("/composition/tempocontroller/{address}"),
                &one
            ),
            Some((target.clone(), Command::Press))
        );
        assert_eq!(
            osc(&project, &format!("/tempocontroller/{address}"), &one),
            Some((target, Command::Press)),
            "also without /composition"
        );
    }
    assert_eq!(
        osc(&project, "/composition/tempocontroller/pause", &one),
        Some((Target::BpmPause, Command::Switch(true)))
    );
    assert_eq!(
        osc(&project, "/composition/decks/2/select", &one),
        Some((
            Target::DeckSelect {
                deck: decks[1].clone()
            },
            Command::Press
        ))
    );
    assert_eq!(
        osc(&project, "/composition/decks/3/select", &one),
        None,
        "no third deck"
    );
    assert_eq!(
        osc(&project, "/composition/decks/next", &one),
        Some((Target::DeckNext, Command::Press))
    );
    assert_eq!(
        osc(&project, "/composition/decks/prev", &one),
        Some((Target::DeckPrev, Command::Press))
    );
    assert_eq!(
        osc(&project, "/composition/disconnectall", &one),
        Some((Target::StopAll, Command::Press))
    );
    assert_eq!(
        osc(
            &project,
            "/composition/dashboard/link8",
            &[OscArg::Float(0.5)]
        ),
        Some((Target::Dashboard { knob: 7 }, Command::Value(0.5)))
    );
    assert_eq!(
        osc(
            &project,
            "/composition/dashboard/link9",
            &[OscArg::Float(0.5)]
        ),
        None
    );
    assert_eq!(
        osc(&project, "/flux/play", &[]),
        None,
        "not a composition address"
    );
}

#[test]
fn the_layers_of_a_global_layer_should_be_found_by_slot() {
    let mut project = show();
    apply(&mut project, VjOp::AddLayer { global: true }).unwrap();
    let layer_id = project.vj.layers[3].id.clone();
    let column = project.vj.decks[0].columns[1].id.clone();
    apply(
        &mut project,
        VjOp::PlaceGenerator {
            generator: crate::generator::Generator::Bars,
            layer: layer_id,
            column,
        },
    )
    .unwrap();
    let clip = project.vj.layers[3].clips[0].id.clone();
    assert_eq!(
        osc(&project, "/composition/layers/4/clips/2/connect", &[]),
        Some((Target::ClipLaunch { clip }, Command::Press))
    );
}
