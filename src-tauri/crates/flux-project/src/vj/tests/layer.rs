use super::*;

#[test]
fn levels_should_be_brought_into_range() {
    let (mut project, mut new_id) = vj_project();
    let op = VjOp::LayerMaster {
        layer: "g1".into(),
        value: 1.5,
    };
    run(&mut project, op, &mut new_id);
    let op = VjOp::LayerTransition {
        layer: "g1".into(),
        value: 30.0,
    };
    run(&mut project, op, &mut new_id);
    let layer = &project.vj.layers[0];
    assert_eq!((layer.master, layer.transition), (1.0, MAX_TRANSITION));
    let op = VjOp::LayerVolume {
        layer: "g1".into(),
        value: f32::NAN,
    };
    assert!(apply(&mut project, op, &mut new_id).is_err());
}

#[test]
fn a_reset_should_bring_a_fader_back_to_its_default() {
    let (mut project, mut new_id) = vj_project();
    let layer = project.vj.layers[0].id.clone();
    for (op, value) in [
        ("layerMaster", 0.2),
        ("layerOpacity", 0.3),
        ("layerVolume", 0.4),
        ("layerTransition", 5.0),
    ] {
        let op =
            serde_json::from_value(serde_json::json!({"op": op, "layer": layer, "value": value}))
                .unwrap();
        run(&mut project, op, &mut new_id);
    }
    for level in [
        Level::Master,
        Level::Opacity,
        Level::Volume,
        Level::Transition,
    ] {
        let op = VjOp::LayerLevelReset {
            layer: layer.clone(),
            level,
        };
        run(&mut project, op, &mut new_id);
    }
    let l = &project.vj.layers[0];
    assert_eq!(
        (l.master, l.opacity, l.volume, l.transition),
        (1.0, 1.0, 1.0, 0.0)
    );
    let op = VjOp::GroupLevelReset {
        group: "nope".into(),
        level: Level::Master,
    };
    assert!(apply(&mut project, op, &mut new_id).is_err());
}

#[test]
fn a_layer_transition_should_be_random_only_when_asked() {
    let (mut project, mut new_id) = vj_project();
    let op = |random| VjOp::LayerTransitionEffect {
        layer: "g1".into(),
        effect: Blend::Add,
        random,
    };
    run(&mut project, op(true), &mut new_id);
    let json = serde_json::to_value(&project.vj.layers[0]).unwrap();
    assert_eq!(json["transitionRandom"], true);
    assert_eq!(json["transitionEffect"], "add");
    run(&mut project, op(false), &mut new_id);
    let json = serde_json::to_value(&project.vj.layers[0]).unwrap();
    assert!(json.get("transitionRandom").is_none());
}

#[test]
fn every_blend_should_be_listed_once_for_the_random_transition() {
    let names: std::collections::HashSet<String> = Blend::ALL
        .iter()
        .map(|b| {
            serde_json::to_value(b)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(names.len(), Blend::ALL.len());
    // A new variant stops this from compiling until it is counted here and in `ALL`.
    let count = |b: Blend| match b {
        Blend::Alpha
        | Blend::Add
        | Blend::Subtract
        | Blend::Multiply
        | Blend::Screen
        | Blend::Lighten
        | Blend::Darken
        | Blend::Difference
        | Blend::Exclusion
        | Blend::Overlay
        | Blend::SoftLight
        | Blend::HardLight
        | Blend::ColorDodge
        | Blend::ColorBurn
        | Blend::LinearLight
        | Blend::VividLight
        | Blend::PinLight
        | Blend::Hue
        | Blend::Saturation
        | Blend::Color
        | Blend::Luminosity
        | Blend::AlphaCut
        | Blend::LumaKey
        | Blend::LumaKeyInverted
        | Blend::LumaIsAlpha
        | Blend::DifferenceInverted
        | Blend::Mask50
        | Blend::Rgb
        | Blend::Displace => 1,
    };
    assert_eq!(Blend::ALL.iter().map(|b| count(*b)).sum::<usize>(), 29);
}

#[test]
fn a_layer_transition_should_take_any_blend_and_default_to_alpha() {
    let (mut project, mut new_id) = vj_project();
    assert_eq!(project.vj.layers[0].transition_effect, Blend::Alpha);
    let op = VjOp::LayerTransitionEffect {
        layer: "g1".into(),
        effect: Blend::Multiply,
        random: false,
    };
    run(&mut project, op, &mut new_id);
    let json = serde_json::to_value(&project.vj.layers[0]).unwrap();
    assert_eq!(json["transitionEffect"], "multiply");
    let back: Layer = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back.transition_effect, Blend::Multiply);
    // Older files have `alpha` or `add`, and none at all for the default.
    let mut older = json;
    older["transitionEffect"] = "add".into();
    let older: Layer = serde_json::from_value(older.clone()).unwrap();
    assert_eq!(older.transition_effect, Blend::Add);
    let mut none = serde_json::to_value(&older).unwrap();
    none.as_object_mut().unwrap().remove("transitionEffect");
    let none: Layer = serde_json::from_value(none).unwrap();
    assert_eq!(none.transition_effect, Blend::Alpha);
}

#[test]
fn validate_should_refuse_levels_out_of_range() {
    let (mut project, _) = vj_project();
    project.vj.layers[0].opacity = 1.5;
    assert!(project.validate().is_err());
    project.vj.layers[0].opacity = 0.5;
    project.vj.layers[0].transition = -1.0;
    assert!(project.validate().is_err());
}

#[test]
fn every_blend_should_keep_its_camel_case_name_and_old_names_should_still_load() {
    for (blend, name) in [
        (Blend::Alpha, "alpha"),
        (Blend::Darken, "darken"),
        (Blend::SoftLight, "softLight"),
        (Blend::ColorDodge, "colorDodge"),
        (Blend::VividLight, "vividLight"),
        (Blend::Luminosity, "luminosity"),
        (Blend::AlphaCut, "alphaCut"),
        (Blend::LumaKey, "lumaKey"),
        (Blend::LumaKeyInverted, "lumaKeyInverted"),
        (Blend::LumaIsAlpha, "lumaIsAlpha"),
        (Blend::DifferenceInverted, "differenceInverted"),
        (Blend::Mask50, "mask50"),
        (Blend::Rgb, "rgb"),
        (Blend::Displace, "displace"),
    ] {
        let json = format!("\"{name}\"");
        assert_eq!(serde_json::to_string(&blend).unwrap(), json);
        assert_eq!(serde_json::from_str::<Blend>(&json).unwrap(), blend);
    }
    assert!(serde_json::from_str::<Blend>("\"nothing\"").is_err());
}

#[test]
fn autopilot_should_pick_next_previous_and_random_skipping_empty_cells() {
    let cells = [true, false, true, true, false];
    let pick = |mode: Autopilot, current, roll| mode.pick(&cells, current, roll);
    assert_eq!(pick(Autopilot::Next, Some(0), 0), Some(2));
    assert_eq!(pick(Autopilot::Next, Some(3), 0), Some(0), "wraps");
    assert_eq!(pick(Autopilot::Previous, Some(2), 0), Some(0));
    assert_eq!(pick(Autopilot::Previous, Some(0), 0), Some(3), "wraps");
    assert_eq!(pick(Autopilot::Next, None, 0), Some(0));
    assert_eq!(pick(Autopilot::Previous, None, 0), Some(3));
    assert_eq!(pick(Autopilot::Off, Some(0), 0), None);
    for roll in 0..8 {
        assert_ne!(pick(Autopilot::Random, Some(2), roll), Some(2));
    }
    assert_eq!(pick(Autopilot::Random, Some(0), 1), Some(3));
    assert_eq!(Autopilot::Next.pick(&[false, true], Some(1), 0), Some(1));
    assert_eq!(Autopilot::Random.pick(&[false, true], Some(1), 5), Some(1));
    assert_eq!(Autopilot::Next.pick(&[false, false], None, 0), None);
    assert_eq!(Autopilot::Next.pick(&[], None, 0), None);
}
