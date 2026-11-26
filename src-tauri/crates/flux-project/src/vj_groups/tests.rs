use super::*;
use crate::events::OscArg;
use crate::vj::{EffectTarget, VjOp, apply};
use crate::vj_feedback::PadState;
use crate::vj_mapping::{Command, Follow, MidiKind, Plan, Source, Target};
use crate::{Project, ProjectMode};
use flux_core::FrameRate;

/// A VJ show with `n` layers, `l0` at the bottom.
fn show(n: usize) -> Project {
    let mut ids = (0..).map(|n| format!("id{n}"));
    let mut project = Project::new("Show", FrameRate::Fps25);
    project.mode = ProjectMode::Vj;
    project.vj = Vj::starting(|| ids.next().unwrap());
    for (i, layer) in project.vj.layers.iter_mut().enumerate() {
        layer.id = format!("l{i}");
    }
    for i in project.vj.layers.len()..n {
        project.vj.layers.push(Layer::new(format!("l{i}"), false));
    }
    project
}

fn run(project: &mut Project, op: VjOp) -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    apply(project, op, || {
        format!("new{}", NEXT.fetch_add(1, Ordering::Relaxed))
    })?;
    project.validate()
}

/// The layers bottom first, those in a group starred.
fn order(project: &Project) -> Vec<String> {
    project
        .vj
        .layers
        .iter()
        .map(|l| match &l.group {
            Some(_) => format!("{}*", l.id),
            None => l.id.clone(),
        })
        .collect()
}

fn group(project: &mut Project, layers: &[&str]) -> String {
    let layers = layers.iter().map(|l| (*l).to_owned()).collect();
    run(project, VjOp::GroupLayers { layers }).unwrap();
    project.vj.groups.last().unwrap().id.clone()
}

#[test]
fn grouping_layers_should_gather_them_where_the_lowest_is() {
    let mut project = show(5);
    let id = group(&mut project, &["l3", "l1"]);
    assert_eq!(order(&project), ["l0", "l1*", "l3*", "l2", "l4"]);
    assert_eq!(project.vj.groups[0].id, id);
    assert_eq!(project.vj.group_layers(&id).count(), 2);
}

#[test]
fn grouping_should_refuse_nothing_and_a_layer_that_is_not_there() {
    let mut project = show(3);
    assert!(run(&mut project, VjOp::GroupLayers { layers: vec![] }).is_err());
    let ghost = VjOp::GroupLayers {
        layers: vec!["ghost".into()],
    };
    assert!(run(&mut project, ghost).is_err());
    assert!(project.vj.groups.is_empty());
}

#[test]
fn a_layer_taken_into_a_new_group_should_leave_its_old_one_and_an_empty_group_should_go() {
    let mut project = show(4);
    group(&mut project, &["l0", "l1"]);
    group(&mut project, &["l1"]);
    assert_eq!(project.vj.groups.len(), 2);
    group(&mut project, &["l0"]);
    assert_eq!(project.vj.groups.len(), 2, "the first group is gone");
    let vj = &project.vj;
    assert!(
        vj.groups
            .iter()
            .all(|g| vj.group_layers(&g.id).count() == 1)
    );
}

#[test]
fn a_layer_should_join_a_group_on_top_and_leave_above_it() {
    let mut project = show(4);
    let id = group(&mut project, &["l1", "l2"]);
    let join = VjOp::LayerGroup {
        layer: "l0".into(),
        group: Some(id),
        to: None,
    };
    run(&mut project, join).unwrap();
    assert_eq!(order(&project), ["l1*", "l2*", "l0*", "l3"]);
    let leave = VjOp::LayerGroup {
        layer: "l1".into(),
        group: None,
        to: None,
    };
    run(&mut project, leave).unwrap();
    assert_eq!(order(&project), ["l2*", "l0*", "l1", "l3"]);
}

#[test]
fn a_layer_dropped_at_a_place_should_go_there_in_its_group() {
    let mut project = show(4);
    let id = group(&mut project, &["l1", "l2"]);
    let drop = VjOp::LayerGroup {
        layer: "l3".into(),
        group: Some(id),
        to: Some(1),
    };
    run(&mut project, drop).unwrap();
    assert_eq!(order(&project), ["l0", "l3*", "l1*", "l2*"]);
}

#[test]
fn the_last_layer_leaving_a_group_should_leave_an_empty_one_in_its_place() {
    let mut project = show(3);
    let id = group(&mut project, &["l1"]);
    let leave = VjOp::LayerGroup {
        layer: "l1".into(),
        group: None,
        to: None,
    };
    run(&mut project, leave).unwrap();
    assert_eq!(project.vj.layers.len(), 4);
    assert_eq!(project.vj.group_layers(&id).count(), 1);
    let fresh = project.vj.group_layers(&id).next().unwrap();
    assert!(fresh.clips.is_empty() && fresh.id != "l1");
}

#[test]
fn ungrouping_should_keep_the_layers_and_drop_the_group() {
    let mut project = show(3);
    let id = group(&mut project, &["l0", "l1"]);
    run(&mut project, VjOp::Ungroup { group: id.clone() }).unwrap();
    assert!(project.vj.groups.is_empty());
    assert_eq!(order(&project), ["l0", "l1", "l2"]);
    assert!(run(&mut project, VjOp::Ungroup { group: id }).is_err());
}

#[test]
fn removing_the_last_layer_of_a_group_should_remove_the_group() {
    let mut project = show(3);
    group(&mut project, &["l1"]);
    run(&mut project, VjOp::RemoveLayer { layer: "l1".into() }).unwrap();
    assert!(project.vj.groups.is_empty());
}

#[test]
fn moving_a_layer_into_a_group_should_join_it_and_moving_it_away_should_leave() {
    let mut project = show(5);
    group(&mut project, &["l1", "l2", "l3"]);
    let to = |layer: &str, to| VjOp::MoveLayer {
        layer: layer.into(),
        to,
    };
    run(&mut project, to("l0", 2)).unwrap();
    assert_eq!(order(&project), ["l1*", "l2*", "l0*", "l3*", "l4"]);
    run(&mut project, to("l0", 5)).unwrap();
    assert_eq!(order(&project), ["l1*", "l2*", "l3*", "l4", "l0"]);
}

#[test]
fn an_edit_should_be_undone_by_the_project_it_started_from() {
    let mut session = crate::Session::default();
    let project = session.new_project_of("Show", FrameRate::Fps25, ProjectMode::Vj);
    let layers: Vec<String> = project.vj.layers.iter().map(|l| l.id.clone()).collect();
    let grouped = session
        .edit_vj(VjOp::GroupLayers { layers })
        .unwrap()
        .project;
    assert_eq!(grouped.vj.groups.len(), 1);
    session.update(project.clone());
    assert_eq!(session.grid_copy().unwrap(), project.vj);
    assert!(session.grid_copy().unwrap().groups.is_empty());
}

#[test]
fn the_group_levels_should_be_brought_into_range_and_the_switches_set() {
    let mut project = show(2);
    let id = group(&mut project, &["l0"]);
    let ops = [
        VjOp::GroupMaster {
            group: id.clone(),
            value: 3.0,
        },
        VjOp::GroupOpacity {
            group: id.clone(),
            value: -1.0,
        },
        VjOp::GroupBypass {
            group: id.clone(),
            on: true,
        },
        VjOp::GroupSolo {
            group: id.clone(),
            on: true,
        },
        VjOp::GroupCollapse {
            group: id.clone(),
            on: true,
        },
        VjOp::GroupBlend {
            group: id.clone(),
            blend: Blend::Add,
        },
        VjOp::RenameGroup {
            group: id.clone(),
            name: " Back ".into(),
        },
    ];
    for op in ops {
        run(&mut project, op).unwrap();
    }
    let g = &project.vj.groups[0];
    assert_eq!((g.master, g.opacity), (1.0, 0.0));
    assert!(g.bypass && g.solo && g.collapsed);
    assert_eq!((g.blend, g.name.as_deref()), (Blend::Add, Some("Back")));
    let nan = VjOp::GroupMaster {
        group: id,
        value: f32::NAN,
    };
    assert!(run(&mut project, nan).is_err());
}

#[test]
fn validation_should_refuse_a_broken_group() {
    let mut project = show(5);
    let id = group(&mut project, &["l1", "l2"]);
    let mut apart = project.clone();
    apart.vj.layers[4].group = Some(id.clone());
    assert!(apart.validate().is_err(), "not next to the others");
    let mut unknown = project.clone();
    unknown.vj.layers[0].group = Some("ghost".into());
    assert!(unknown.validate().is_err());
    let mut empty = project.clone();
    empty.vj.groups.push(Group::new("lonely".into()));
    assert!(empty.validate().is_err(), "a group has a layer");
    let mut repeated = project.clone();
    repeated.vj.groups.push(Group::new(id));
    assert!(repeated.validate().is_err());
    let mut loud = project.clone();
    loud.vj.groups[0].opacity = 1.5;
    assert!(loud.validate().is_err());
    let mut clash = project.clone();
    clash.vj.groups[0].id = "l0".into();
    for layer in &mut clash.vj.layers {
        if layer.group.is_some() {
            layer.group = Some("l0".into());
        }
    }
    assert!(clash.validate().is_err(), "the id of a layer");
    project.validate().unwrap();
}

#[test]
fn a_group_should_take_effects_but_not_the_ones_for_the_composition() {
    let mut project = show(2);
    let id = group(&mut project, &["l0"]);
    let add = |kind: &str| VjOp::AddEffect {
        target: EffectTarget::Group(id.clone()),
        kind: kind.into(),
    };
    run(&mut project, add("blur")).unwrap();
    assert!(run(&mut project, add("feedback")).is_err());
    let effect = project.vj.groups[0].effects[0].id.clone();
    assert!(project.vj.find_effect(&effect).is_some());
    run(&mut project, VjOp::RemoveEffect { effect }).unwrap();
    assert!(project.vj.groups[0].effects.is_empty());
}

#[test]
fn a_solo_on_a_group_should_show_its_layers_and_one_on_a_layer_only_that_layer() {
    let mut project = show(4);
    let id = group(&mut project, &["l1", "l2"]);
    let vj = &mut project.vj;
    assert!(vj.layers.iter().all(|l| vj.solo_lets(l)));
    vj.group_mut(&id).unwrap().solo = true;
    let lets: Vec<bool> = vj.layers.iter().map(|l| vj.solo_lets(l)).collect();
    assert_eq!(lets, [false, true, true, false]);
    vj.group_mut(&id).unwrap().solo = false;
    vj.layers[1].solo = true;
    let lets: Vec<bool> = vj.layers.iter().map(|l| vj.solo_lets(l)).collect();
    assert_eq!(lets, [false, true, false, false]);
}

#[test]
fn a_file_without_groups_should_open_with_none_and_one_with_groups_should_keep_them() {
    let old = r#"{"layers":[{"id":"a"},{"id":"b"}],"decks":[],"deck":""}"#;
    let vj: Vj = serde_json::from_str(old).unwrap();
    assert!(vj.groups.is_empty() && vj.layers.iter().all(|l| l.group.is_none()));
    let json = serde_json::to_value(&vj).unwrap();
    assert!(json.get("groups").is_none() && json["layers"][0].get("group").is_none());
    let mut project = show(3);
    group(&mut project, &["l0", "l1"]);
    let json = serde_json::to_string(&project.vj).unwrap();
    let back: Vj = serde_json::from_str(&json).unwrap();
    assert_eq!(back, project.vj);
}

fn midi(number: u8) -> Source {
    Source::Midi {
        port: Some("APC40 mkII".into()),
        channel: 1,
        kind: MidiKind::Note,
        number,
    }
}

#[test]
fn a_group_should_answer_to_resolumes_osc_addresses_by_its_place() {
    let mut project = show(4);
    let first = group(&mut project, &["l0", "l1"]);
    let second = group(&mut project, &["l3"]);
    let vj = &project.vj;
    let osc = |address: &str, value: f32| {
        crate::vj_mapping::osc_command(vj, address, &[OscArg::Float(value)])
    };
    assert_eq!(
        osc("/composition/groups/1/master", 0.5),
        Some((
            Target::GroupMaster {
                group: first.clone()
            },
            Command::Value(0.5)
        ))
    );
    assert_eq!(
        osc("/composition/groups/2/video/opacity", 0.25),
        Some((
            Target::GroupOpacity {
                group: second.clone()
            },
            Command::Value(0.25)
        ))
    );
    assert_eq!(
        osc("/composition/groups/1/bypassed", 1.0),
        Some((Target::GroupBypass { group: first }, Command::Switch(true)))
    );
    assert_eq!(
        osc("/composition/groups/2/solo", 0.0),
        Some((Target::GroupSolo { group: second }, Command::Switch(false)))
    );
    assert!(osc("/composition/groups/3/master", 0.5).is_none());
}

#[test]
fn group_targets_should_edit_the_group_and_light_the_pad() {
    let mut project = show(3);
    let id = group(&mut project, &["l0"]);
    let bypass = Target::GroupBypass { group: id.clone() };
    let master = Target::GroupMaster { group: id.clone() };
    for (n, target) in [bypass.clone(), master.clone()].into_iter().enumerate() {
        let source = midi(n as u8 + 1);
        run(&mut project, VjOp::AddMapping { source, target }).unwrap();
    }
    let vj = &project.vj;
    assert_eq!(
        crate::vj_mapping::plan(vj, &bypass, Command::Toggle),
        Some(Plan::Edit(VjOp::GroupBypass {
            group: id.clone(),
            on: true
        }))
    );
    assert_eq!(
        crate::vj_mapping::plan(vj, &master, Command::Value(0.3)),
        Some(Plan::Edit(VjOp::GroupMaster {
            group: id.clone(),
            value: 0.3
        }))
    );
    assert_eq!(master.range(vj), Some((0.0, 1.0)));
    let lit = |vj: &Vj| crate::vj_feedback::pads(vj, &|_| false, false);
    assert_eq!(lit(vj)[0].state, PadState::Off);
    let on = VjOp::GroupBypass {
        group: id,
        on: true,
    };
    run(&mut project, on).unwrap();
    assert_eq!(lit(&project.vj)[0].state, PadState::On);
}

#[test]
fn a_group_mapping_should_follow_its_place_when_the_group_changes() {
    let mut project = show(3);
    let id = group(&mut project, &["l0"]);
    let add = VjOp::AddMapping {
        source: midi(1),
        target: Target::GroupMaster { group: id.clone() },
    };
    run(&mut project, add).unwrap();
    assert_eq!(project.vj.mappings[0].follow, Follow::Position);
    run(&mut project, VjOp::Ungroup { group: id }).unwrap();
    assert!(project.vj.mappings[0].parked, "no group fills the slot");
    let again = group(&mut project, &["l1"]);
    let m = &project.vj.mappings[0];
    assert!(!m.parked);
    assert_eq!(m.target, Target::GroupMaster { group: again });
}
