use super::{
    MASKS_SIZE, Mask, Quad, SHADER, Sizing, VERTEX_SIZE, fit, fitted_quad, mask_bytes, placement,
    transformed, vertex_bytes, vertices,
};

#[test]
fn placement_should_size_the_picture_into_the_composition() {
    let (wide, full) = ((1920, 1080), (3840, 1080));
    assert_eq!(placement(Sizing::Stretch, wide, full), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(placement(Sizing::Fit, wide, full), [0.25, 0.0, 0.5, 1.0]);
    assert_eq!(
        placement(Sizing::None, (960, 540), full),
        [0.375, 0.25, 0.25, 0.5]
    );
    let fill = placement(Sizing::Fill, wide, full);
    assert_eq!((fill[0], fill[2]), (0.0, 1.0));
    assert!((fill[3] - 2.0).abs() < 1e-6 && (fill[1] + 0.5).abs() < 1e-6);
}

#[test]
fn transformed_should_scale_about_the_centre_and_move_it() {
    let place = [0.25, 0.0, 0.5, 1.0];
    assert_eq!(transformed(place, 1.0, [0.0, 0.0]), place);
    assert_eq!(transformed(place, 2.0, [0.0, 0.0]), [0.0, -0.5, 1.0, 2.0]);
    assert_eq!(
        transformed(place, 0.5, [0.25, -0.25]),
        [0.625, 0.0, 0.25, 0.5]
    );
}

#[test]
fn the_shader_should_parse_and_validate() {
    let module = naga::front::wgsl::parse_str(SHADER).expect("the shader parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .expect("the shader validates");
}

#[test]
fn the_shader_should_know_every_kind_of_step_and_size_the_uniform_as_the_program() {
    use super::{EFFECTS_SIZE, MAX_LAYERS, effects};
    let kinds = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 20, 21, 22, 23, 24, 30, 31, 32,
        33, 34, 35, 36, 37, 38,
    ];
    for kind in kinds {
        assert!(SHADER.contains(&format!("kind == {kind}u")), "kind {kind}");
    }
    assert!(SHADER.contains(&format!("array<vec4f, {}>", effects::MAX_STEPS * 4)));
    assert!(SHADER.contains(&format!("array<Fx, {MAX_LAYERS}>")));
    // A uniform binding holds 64 KiB at least on every GPU wgpu runs on by default.
    const { assert!(EFFECTS_SIZE <= 65_536) };
    assert_eq!(effects::FX_FLOATS * 4, 16 + 16 * 4 * effects::MAX_STEPS);
}

#[test]
fn fit_should_letterbox_a_wide_picture_and_pillarbox_a_tall_one() {
    assert_eq!(fit((1920, 1080), (1920, 1200)), (0.0, 60.0, 1920.0, 1080.0));
    assert_eq!(
        fit((1080, 1080), (1920, 1080)),
        (420.0, 0.0, 1080.0, 1080.0)
    );
    assert_eq!(fit((1280, 720), (1920, 1080)), (0.0, 0.0, 1920.0, 1080.0));
}

#[test]
fn a_rectangle_should_have_no_perspective() {
    let quad = fitted_quad((1080, 1080), (1920, 1080));
    assert_eq!(quad.corners[0], [420.0 / 1920.0, 0.0]);
    for v in vertices(&quad) {
        assert!((v.uvq[2] - 2.0).abs() < 1e-5, "{v:?}");
    }
}

#[test]
fn a_trapezoid_should_weigh_its_wide_side_more() {
    // The top edge is half as wide as the bottom: the picture is seen leaning away.
    let quad = Quad {
        input: [0.0, 0.0, 1.0, 1.0],
        corners: [[0.25, 0.0], [0.75, 0.0], [1.0, 1.0], [0.0, 1.0]],
        soft: [0.0; 4],
        gamma: 1.0,
    };
    let v = vertices(&quad);
    // The narrow top is far away: its corners (0, 1) weigh less than the bottom's (2, 3).
    assert!(v[0].uvq[2] < v[2].uvq[2]);
    // Divided out, every corner still samples its own corner of the picture.
    let uv = |i: usize| [v[i].uvq[0] / v[i].uvq[2], v[i].uvq[1] / v[i].uvq[2]];
    assert_eq!(
        (uv(0), uv(1), uv(2), uv(5)),
        ([0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0])
    );
}

#[test]
fn the_masks_should_fill_their_uniform_exactly() {
    let square = Mask {
        points: vec![[0.1, 0.1], [0.2, 0.1], [0.2, 0.2], [0.1, 0.2]],
        invert: true,
    };
    let line = Mask {
        points: vec![[0.0, 0.0], [1.0, 1.0]],
        invert: false,
    };
    let bytes = mask_bytes(&[line, square], (1920, 1080));
    assert_eq!(bytes.len() as u64, MASKS_SIZE);
    let float = |at: usize| f32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap());
    let int = |at: usize| u32::from_ne_bytes(bytes[at..at + 4].try_into().unwrap());
    // A line is no shape: only the square is drawn, from the first point, inverted.
    assert_eq!((float(0), float(4), float(8)), (1920.0, 1080.0, 1.0));
    assert_eq!((int(16), int(20), int(24)), (0, 4, 1));
    assert_eq!(float(16 + 16 * 8 + 8), 0.2);
}

#[test]
fn a_vertex_should_carry_its_place_in_the_quad() {
    let quad = Quad {
        soft: [0.25, 0.0, 0.0, 2.0],
        gamma: 2.2,
        ..fitted_quad((1920, 1080), (1920, 1080))
    };
    let v = vertices(&quad);
    assert_eq!(
        [v[2].local[0] / v[2].uvq[2], v[2].local[1] / v[2].uvq[2]],
        [1.0, 1.0]
    );
    // A fade past the whole quad is cut to it.
    assert_eq!((v[0].soft, v[0].gamma), ([0.25, 0.0, 0.0, 1.0], 2.2));
    assert_eq!(vertex_bytes(&v).len(), 6 * 19 * 4);
    assert_eq!(VERTEX_SIZE, 19 * 4);
}
