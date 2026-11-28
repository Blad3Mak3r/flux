//! Putting the mappings of a file into a show.

use super::*;
use crate::vj::Vj;
use crate::vj_mapping::{Follow, Target, add_mapping, link_knob, rename_knob, set_mapping};

/// Gives the mapping just added the mode and slot the file says (the default of a new one
/// otherwise stands, for a file without them).
pub(super) fn keep_follow(vj: &mut Vj, id: &str, incoming: &FileMapping) -> bool {
    let Some(mapping) = vj.mappings.iter_mut().find(|m| m.id == id) else {
        return false;
    };
    if kind_of(&mapping.target) != Kind::Plain {
        mapping.follow = incoming.follow;
        mapping.place = (incoming.follow == Follow::Position).then(|| incoming.target.clone());
    }
    true
}

/// Where an incoming mapping lands in `vj`: its id if the show has it, else its position.
pub(super) fn resolve(vj: &Vj, incoming: &FileMapping) -> Option<Target> {
    incoming
        .ids
        .clone()
        .filter(|ids| ids.exists(vj))
        .or_else(|| from_places(vj, &incoming.target))
}

/// What importing `file` into `vj` would do: how much resolves, and the mappings it would overwrite.
pub fn plan(vj: &Vj, file: &MapFile) -> ImportPlan {
    let mut plan = ImportPlan::default();
    for (index, incoming) in file.mappings.iter().enumerate() {
        let Some(target) = resolve(vj, incoming) else {
            plan.skipped += 1;
            continue;
        };
        plan.taken += 1;
        let existing = vj
            .mappings
            .iter()
            .find(|m| m.source == incoming.source || m.target == target);
        if let Some(existing) = existing {
            plan.conflicts.push(Conflict {
                index,
                source: incoming.source.clone(),
                existing: existing.target.clone(),
                incoming: target,
            });
        }
    }
    for link in file.dashboard.iter().flat_map(|knob| &knob.links) {
        if from_places(vj, &link.target).is_some() {
            plan.taken += 1;
        } else {
            plan.skipped += 1;
        }
    }
    plan
}

/// Puts the mappings and links of `file` into `vj`, looking each place up in it. A mapping or
/// link that does not resolve or does not fit is counted as skipped.
pub fn import(
    vj: &mut Vj,
    file: &MapFile,
    choice: &ImportChoice,
    mut new_id: impl FnMut() -> String,
) -> ImportReport {
    let mut report = ImportReport::default();
    let conflicts = if choice.wipe {
        Vec::new()
    } else {
        plan(vj, file).conflicts
    };
    if choice.wipe {
        vj.mappings.clear();
        vj.dashboard.clear();
    }
    for (index, incoming) in file.mappings.iter().enumerate() {
        if choice.keep.contains(&index) && conflicts.iter().any(|c| c.index == index) {
            report.kept += 1;
            continue;
        }
        let done = resolve(vj, incoming).is_some_and(|target| {
            let id = new_id();
            add_mapping(vj, incoming.source.clone(), target, id.clone()).is_ok()
                && keep_follow(vj, &id, incoming)
                && {
                    if let Some(m) = vj.mappings.iter_mut().find(|m| m.id == id) {
                        m.lights = incoming.lights;
                    }
                    true
                }
                && (set_mapping(
                    vj,
                    &id,
                    incoming.min,
                    incoming.max,
                    incoming.invert,
                    incoming.mode,
                )
                .is_ok()
                    || {
                        vj.mappings.retain(|m| m.id != id);
                        false
                    })
        });
        if done {
            report.imported += 1;
        } else {
            report.skipped += 1;
        }
    }
    for (knob, incoming) in file
        .dashboard
        .iter()
        .enumerate()
        .take(crate::vj_mapping::KNOBS)
    {
        if incoming.name.is_some() && rename_knob(vj, knob, incoming.name.clone()).is_err() {
            report.skipped += 1;
        }
        for link in &incoming.links {
            let linked = from_places(vj, &link.target).is_some_and(|target| {
                link_knob(vj, knob, target, Some(link.min), Some(link.max)).is_ok()
            });
            if linked {
                report.imported += 1;
            } else {
                report.skipped += 1;
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vj::{EffectTarget, VjOp};
    use crate::vj_mapfile::support::*;

    use crate::vj_mapping::Target;

    #[test]
    fn a_file_should_carry_mappings_and_links_into_a_show_with_other_ids() {
        let from = mapped();
        let text = export(&from.vj).unwrap();
        let mut into = show("b");
        let layer = into.vj.layers[1].id.clone();
        apply(
            &mut into,
            VjOp::AddEffect {
                target: EffectTarget::Layer(layer),
                kind: "blur".into(),
            },
        );

        let report = import(&mut into.vj, &parse(&text).unwrap(), &wipe(), ids());

        assert_eq!(
            report,
            ImportReport {
                imported: 6,
                skipped: 0,
                kept: 0
            }
        );
        into.validate().unwrap();
        let target_of = |source: &Source| {
            into.vj
                .mappings
                .iter()
                .find(|m| m.source == *source)
                .unwrap()
                .clone()
        };
        let opacity = target_of(&cc(1));
        assert_eq!(
            opacity.target,
            Target::LayerOpacity {
                layer: into.vj.layers[1].id.clone()
            }
        );
        assert_eq!((opacity.min, opacity.max, opacity.invert), (0.2, 0.9, true));
        assert_eq!(
            target_of(&Source::Artnet {
                universe: 1,
                channel: 37,
                fine: Some(38)
            })
            .target,
            Target::EffectParam {
                effect: into.vj.layers[1].effects[0].id.clone(),
                param: "radius".into()
            }
        );
        assert_eq!(
            target_of(&Source::Osc {
                address: "/go".into()
            })
            .target,
            Target::LayerBypass {
                layer: into.vj.layers[1].id.clone()
            }
        );
        let knob = &into.vj.dashboard[3];
        assert_eq!(knob.name.as_deref(), Some("Fade"));
        assert_eq!(
            knob.links[0].target,
            Target::LayerMaster {
                layer: into.vj.layers[2].id.clone()
            }
        );
        assert_eq!((knob.links[0].min, knob.links[0].max), (0.1, 0.8));
    }

    #[test]
    fn what_does_not_resolve_should_be_skipped_and_counted() {
        let text = export(&mapped().vj).unwrap();
        // No effect on the layer, so its setting is gone; one deck with columns, but fewer layers.
        let mut into = show("b");
        into.vj.layers.truncate(1);

        let report = import(&mut into.vj, &parse(&text).unwrap(), &wipe(), ids());

        // Only the crossfader and the column launch fit; the link and four mappings do not.
        assert_eq!(
            report,
            ImportReport {
                imported: 2,
                skipped: 4,
                kept: 0
            }
        );
        assert_eq!(into.vj.mappings.len(), 2);
        into.validate().unwrap();
    }

    #[test]
    fn merge_should_replace_the_same_source_and_keep_the_rest() {
        let text = export(&mapped().vj).unwrap();
        let mut into = show("b");
        let old = into.vj.layers[0].id.clone();
        let kept = Target::LayerMaster { layer: old.clone() };
        apply(
            &mut into,
            VjOp::AddMapping {
                source: cc(9),
                target: kept.clone(),
            },
        );
        // The same source as the file's cc 1, on another target.
        apply(
            &mut into,
            VjOp::AddMapping {
                source: cc(1),
                target: Target::LayerVolume { layer: old },
            },
        );

        import(
            &mut into.vj,
            &parse(&text).unwrap(),
            &ImportChoice::default(),
            ids(),
        );

        assert!(
            into.vj
                .mappings
                .iter()
                .any(|m| m.source == cc(9) && m.target == kept)
        );
        let ones: Vec<_> = into
            .vj
            .mappings
            .iter()
            .filter(|m| m.source == cc(1))
            .collect();
        assert_eq!(ones.len(), 1, "the incoming one replaced the old");
        assert_eq!(
            ones[0].target,
            Target::LayerOpacity {
                layer: into.vj.layers[1].id.clone()
            }
        );
    }

    #[test]
    fn replace_should_drop_what_the_show_had() {
        let text = export(&mapped().vj).unwrap();
        let mut into = show("b");
        let layer = into.vj.layers[0].id.clone();
        apply(
            &mut into,
            VjOp::AddMapping {
                source: cc(9),
                target: Target::LayerMaster { layer },
            },
        );

        import(&mut into.vj, &parse(&text).unwrap(), &wipe(), ids());

        assert!(!into.vj.mappings.iter().any(|m| m.source == cc(9)));
    }

    #[test]
    fn the_plan_should_list_what_would_be_overwritten_and_touch_nothing() {
        let into = existing();

        let plan = plan(&into.vj, &file());

        assert_eq!((plan.taken, plan.skipped), (3, 0));
        let found: Vec<_> = plan
            .conflicts
            .iter()
            .map(|c| (c.index, c.source.clone()))
            .collect();
        assert_eq!(
            found,
            vec![(0, cc(1)), (1, cc(7))],
            "the same source, and the same target"
        );
        assert_eq!(
            plan.conflicts[0].existing,
            Target::LayerMaster {
                layer: into.vj.layers[0].id.clone()
            }
        );
        assert_eq!(
            plan.conflicts[0].incoming,
            Target::LayerOpacity {
                layer: into.vj.layers[2].id.clone()
            }
        );
        assert_eq!(into.vj.mappings.len(), 2);
    }

    #[test]
    fn a_show_with_nothing_to_overwrite_should_have_no_conflicts() {
        assert!(plan(&show("b").vj, &file()).conflicts.is_empty());
    }

    #[test]
    fn overwrite_all_should_take_every_incoming_mapping_and_keep_the_rest() {
        let mut into = existing();

        let report = import(&mut into.vj, &file(), &ImportChoice::default(), ids());

        assert_eq!((report.imported, report.kept), (3, 0));
        assert_eq!(
            sources(&into),
            vec![cc(1), cc(7), cc(8)]
                .into_iter()
                .chain([])
                .collect::<Vec<_>>()
        );
        into.validate().unwrap();
    }

    #[test]
    fn keep_existing_should_import_only_what_does_not_conflict() {
        let mut into = existing();
        let keep = ImportChoice {
            wipe: false,
            keep: vec![0, 1],
        };

        let report = import(&mut into.vj, &file(), &keep, ids());

        assert_eq!((report.imported, report.kept), (1, 2));
        assert_eq!(sources(&into), vec![cc(1), cc(2), cc(8)]);
        assert_eq!(
            into.vj
                .mappings
                .iter()
                .find(|m| m.source == cc(1))
                .unwrap()
                .target,
            Target::LayerMaster {
                layer: into.vj.layers[0].id.clone()
            },
            "the show's own mapping stands"
        );
    }

    #[test]
    fn a_choice_per_conflict_should_keep_one_and_overwrite_the_other() {
        let mut into = existing();
        let keep = ImportChoice {
            wipe: false,
            keep: vec![1],
        };

        let report = import(&mut into.vj, &file(), &keep, ids());

        assert_eq!((report.imported, report.kept), (2, 1));
        assert!(into.vj.mappings.iter().any(|m| m.source == cc(2)), "kept");
        assert_eq!(
            into.vj
                .mappings
                .iter()
                .find(|m| m.source == cc(1))
                .unwrap()
                .target,
            Target::LayerOpacity {
                layer: into.vj.layers[2].id.clone()
            },
            "overwritten"
        );
    }

    #[test]
    fn replace_all_should_leave_only_the_files_mappings() {
        let mut into = existing();

        let report = import(&mut into.vj, &file(), &wipe(), ids());

        assert_eq!((report.imported, report.kept), (3, 0));
        assert_eq!(sources(&into), vec![cc(1), cc(7), cc(8)]);
    }

    #[test]
    fn a_keep_that_names_no_conflict_should_change_nothing() {
        let mut into = show("b");
        let keep = ImportChoice {
            wipe: false,
            keep: vec![0, 1, 2],
        };

        let report = import(&mut into.vj, &file(), &keep, ids());

        assert_eq!((report.imported, report.kept), (3, 0));
    }

    #[test]
    fn a_file_read_back_into_its_own_show_should_find_its_ids_first() {
        let mut project = mapped();
        let text = export(&project.vj).unwrap();
        // The layers are reordered since the export: by id the mapping still finds its layer.
        let layer = project.vj.layers[1].id.clone();
        apply(
            &mut project,
            VjOp::MoveLayer {
                layer: layer.clone(),
                to: 0,
            },
        );
        let id = project
            .vj
            .mappings
            .iter()
            .find(|m| m.source == cc(2))
            .unwrap()
            .id
            .clone();
        apply(&mut project, VjOp::RemoveMapping { mapping: id });
        let by_id = Target::LayerBypass {
            layer: layer.clone(),
        };

        let mut into = project.clone();
        import(
            &mut into.vj,
            &parse(&text).unwrap(),
            &ImportChoice::default(),
            ids(),
        );

        let found = into
            .vj
            .mappings
            .iter()
            .find(|m| {
                m.source
                    == Source::Osc {
                        address: "/go".into(),
                    }
            })
            .unwrap();
        assert_eq!(
            found.target, by_id,
            "the layer by id, though it is no longer at its position"
        );
    }

    #[test]
    fn pad_colours_should_travel_in_the_file_and_an_old_file_should_have_none() {
        let mut from = mapped();
        let id = from.vj.mappings[0].id.clone();
        let lights = crate::vj_feedback::Lights {
            off: 0,
            loaded: 5,
            on: 1,
        };
        apply(
            &mut from,
            VjOp::SetMappingLights {
                mapping: id,
                lights: Some(lights),
            },
        );
        let source = from.vj.mappings[0].source.clone();
        let text = export(&from.vj).unwrap();

        let mut into = show("b");
        import(&mut into.vj, &parse(&text).unwrap(), &wipe(), ids());
        let got = into
            .vj
            .mappings
            .iter()
            .find(|m| m.source == source)
            .unwrap();
        assert_eq!(got.lights, Some(lights));

        let old = text.replace("\"lights\"", "\"unknown\"");
        let mut into = show("c");
        import(&mut into.vj, &parse(&old).unwrap(), &wipe(), ids());
        assert!(into.vj.mappings.iter().all(|m| m.lights.is_none()));
    }
}
