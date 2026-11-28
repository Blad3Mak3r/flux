use super::edit::linkable;
use super::{Class, Target};
use crate::model::{MAX_BPM, MIN_BPM};
use crate::vj::{MAX_SPEED, MAX_TRANSITION, MIN_SPEED};
use serde::Serialize;

/// What the window needs to know of one kind of target without a target in hand: how it is
/// driven, whether a knob can link it and the range of its number.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetKind {
    pub kind: String,
    pub class: Class,
    pub linkable: bool,
    /// The range of the number a continuous target takes; absent when it depends on the target's
    /// own setting (an effect parameter, a generator number).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<(f64, f64)>,
}

/// The kinds of target that are not plain actions, for the window to classify targets by `kind`.
pub fn target_kinds() -> Vec<TargetKind> {
    let s = String::new;
    let unit = Some((0.0, 1.0));
    let samples = [
        (Target::LayerBypass { layer: s() }, None),
        (Target::LayerSolo { layer: s() }, None),
        (Target::GroupBypass { group: s() }, None),
        (Target::GroupSolo { group: s() }, None),
        (Target::EffectBypass { effect: s() }, None),
        (Target::BpmPause, None),
        (Target::LayerMaster { layer: s() }, unit),
        (Target::LayerVolume { layer: s() }, unit),
        (Target::LayerOpacity { layer: s() }, unit),
        (Target::GroupMaster { group: s() }, unit),
        (Target::GroupOpacity { group: s() }, unit),
        (
            Target::LayerTransition { layer: s() },
            Some((0.0, f64::from(MAX_TRANSITION))),
        ),
        (Target::Crossfader, unit),
        (Target::Bpm, Some((MIN_BPM, MAX_BPM))),
        (
            Target::ClipSpeed { clip: s() },
            Some((f64::from(MIN_SPEED), f64::from(MAX_SPEED))),
        ),
        (
            Target::EffectParam {
                effect: s(),
                param: s(),
            },
            None,
        ),
        (
            Target::GeneratorNumber {
                clip: s(),
                name: s(),
            },
            None,
        ),
        (Target::Dashboard { knob: 0 }, unit),
        (
            Target::LayerDashboard {
                layer: s(),
                knob: 0,
            },
            unit,
        ),
        (Target::ClipDashboard { clip: s(), knob: 0 }, unit),
    ];
    samples
        .into_iter()
        .map(|(target, range)| TargetKind {
            kind: serde_json::to_value(&target)
                .ok()
                .and_then(|v| v.get("kind")?.as_str().map(str::to_owned))
                .unwrap_or_default(),
            class: target.class(),
            linkable: linkable(&target),
            range,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(name: &str) -> TargetKind {
        target_kinds()
            .into_iter()
            .find(|k| k.kind == name)
            .unwrap_or_else(|| panic!("no kind {name}"))
    }

    #[test]
    fn every_kind_should_be_named_and_listed_once() {
        let kinds = target_kinds();
        assert!(kinds.iter().all(|k| !k.kind.is_empty()));
        let mut names: Vec<_> = kinds.iter().map(|k| k.kind.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), kinds.len());
    }

    #[test]
    fn classes_and_links_should_match_the_targets() {
        assert_eq!(kind("layerBypass").class, Class::Switch);
        assert_eq!(kind("layerOpacity").class, Class::Continuous);
        assert!(kind("layerMaster").linkable);
        assert!(!kind("dashboard").linkable);
        assert!(!kind("crossfader").linkable);
        assert!(!kind("layerBypass").linkable);
    }

    #[test]
    fn the_ranges_should_be_those_the_backend_clamps_to() {
        assert_eq!(kind("bpm").range, Some((MIN_BPM, MAX_BPM)));
        assert_eq!(kind("layerTransition").range, Some((0.0, 10.0)));
        assert_eq!(kind("effectParam").range, None);
    }
}
