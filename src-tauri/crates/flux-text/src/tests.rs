use super::*;

fn block(text: &str) -> Block<'_> {
    Block {
        text,
        family: BUNDLED_FAMILY,
        style: "Regular",
        size: 40.0,
        colour: [255, 128, 0],
        opacity: 1.0,
        align: Align::Center,
        centre: [100.0, 50.0],
        rotation: 0.0,
        tracking: 0.0,
        leading: 0.0,
    }
}

fn draw(block: &Block<'_>) -> Vec<u8> {
    // The bundled face, so the test needs no font of the machine.
    rasterise_with(&bundled(400), block, 200, 100)
}

/// The columns (first, last) with any glyph, over the rows from `from` to `to`.
fn extent(picture: &[u8], rows: std::ops::Range<usize>) -> Option<(usize, usize)> {
    let lit = |x: usize| rows.clone().any(|y| picture[(y * 200 + x) * 4 + 3] > 0);
    let first = (0..200).find(|&x| lit(x))?;
    Some((first, (0..200).rev().find(|&x| lit(x))?))
}

fn covered(picture: &[u8]) -> usize {
    picture.chunks(4).filter(|p| p[3] > 0).count()
}

#[test]
fn the_bundled_font_should_draw_accents_and_keep_the_colour_everywhere() {
    let picture = draw(&block("Canción"));
    assert_eq!(picture.len(), 200 * 100 * 4);
    assert!(covered(&picture) > 200, "some glyphs");
    assert!(picture.chunks(4).all(|p| p[..3] == [255, 128, 0]));
    let plain = draw(&block("Cancion"));
    assert_ne!(picture, plain, "the accent is drawn");
}

#[test]
fn different_text_should_draw_different_pixels() {
    assert_ne!(draw(&block("one")), draw(&block("two")));
    assert_eq!(
        draw(&block("one")),
        draw(&block("one")),
        "and the same text the same"
    );
}

#[test]
fn empty_text_and_no_opacity_should_draw_nothing() {
    assert_eq!(covered(&draw(&block(" \n "))), 0);
    let mut clear = block("text");
    clear.opacity = 0.0;
    assert_eq!(covered(&draw(&clear)), 0);
}

#[test]
fn align_should_move_the_short_line_against_the_long_one() {
    let text = "WWWWWWWW\nii";
    // The second line is in the lower half of the block, below the centre row of 50.
    let lower = 50..100;
    let small = |align| Block {
        size: 16.0,
        align,
        ..block(text)
    };
    let (left, centre, right) = (
        small(Align::Left),
        small(Align::Center),
        small(Align::Right),
    );
    let first = |b: &Block<'_>| extent(&draw(b), lower.clone()).unwrap().0;
    let (l, c, r) = (first(&left), first(&centre), first(&right));
    assert!(l < c && c < r, "{l} {c} {r}");
}

#[test]
fn position_and_size_should_move_and_grow_the_block() {
    let mut moved = block("W");
    moved.centre = [150.0, 50.0];
    let (a, b) = (
        extent(&draw(&block("W")), 0..100).unwrap(),
        extent(&draw(&moved), 0..100).unwrap(),
    );
    assert!(b.0 > a.0 + 40, "{a:?} {b:?}");
    let mut big = block("W");
    big.size = 80.0;
    assert!(covered(&draw(&big)) > covered(&draw(&block("W"))) * 2);
}

#[test]
fn tracking_should_widen_a_line_and_leading_should_tall_the_block() {
    let mut wide = block("WWW");
    wide.tracking = 0.5;
    let (a, b) = (
        extent(&draw(&block("WWW")), 0..100).unwrap(),
        extent(&draw(&wide), 0..100).unwrap(),
    );
    assert!(b.1 - b.0 > a.1 - a.0 + 20);
    let rows = |picture: &[u8]| {
        let lit: Vec<usize> = (0..100)
            .filter(|&y| (0..200).any(|x| picture[(y * 200 + x) * 4 + 3] > 0))
            .collect();
        lit.last().unwrap() - lit.first().unwrap()
    };
    let two = Block {
        size: 16.0,
        ..block("W\nW")
    };
    let loose = Block {
        leading: 0.5,
        ..two.clone()
    };
    assert!(rows(&draw(&loose)) > rows(&draw(&two)) + 5);
}

#[test]
fn rotation_should_turn_a_wide_block_tall() {
    let mut turned = block("WWWWWW");
    turned.rotation = 90.0;
    let flat = draw(&block("WWWWWW"));
    let upright = draw(&turned);
    let width = |p: &[u8]| {
        let (a, b) = extent(p, 0..100).unwrap();
        b - a
    };
    assert!(width(&upright) < width(&flat) / 2);
}

#[test]
fn a_missing_family_should_fall_back_to_the_bundled_font() {
    let font = font("No Such Family 7", "Bold");
    assert!(font.has('A'));
    assert!(families().iter().any(|f| f.name == BUNDLED_FAMILY));
}

fn width_of(text: &str, tracking: f32) -> f32 {
    let face = bundled(400);
    layout_line(&face, (400, Style::Normal), 100.0, text, tracking).width
}

#[test]
fn shaping_should_kern_pairs_the_font_kerns_in_gpos() {
    let (pair, apart) = (width_of("AV", 0.0), width_of("A", 0.0) + width_of("V", 0.0));
    assert!(pair < apart - 1.0, "{pair} is not under {apart}");
}

#[test]
fn tracking_should_still_widen_a_shaped_line() {
    let loose = width_of("AVA", 10.0);
    assert!(
        (loose - (width_of("AVA", 0.0) + 20.0)).abs() < 0.5,
        "{loose}"
    );
}

#[test]
fn runs_should_split_where_the_face_changes_and_keep_joiners_with_what_they_follow() {
    let face = |c: char| u8::from(c.is_ascii());
    let runs = split_runs("ab\u{416}\u{200D}c", face);
    assert_eq!(runs, vec![(1, "ab"), (0, "\u{416}\u{200D}"), (1, "c")]);
    assert!(split_runs("", face).is_empty());
}

#[test]
fn a_character_no_font_has_should_not_stop_the_line_and_keep_the_others_in_place() {
    // A private use character: no face has it, so it draws as a blank or the face's own gap.
    let plain = width_of("AB", 0.0);
    let with = width_of("A\u{10FFFD}B", 0.0);
    assert!(with >= plain, "{with} {plain}");
    assert!(with < plain + 200.0, "{with} {plain}");
    let mut text = block("A\u{10FFFD}B \u{416}\u{2605}\u{1F389}");
    text.size = 20.0;
    assert!(covered(&draw(&text)) > 0);
}

#[test]
fn a_character_the_chosen_face_lacks_should_fall_through_to_a_face_that_has_it() {
    // The bundled subset is Latin only: Cyrillic is absent, so another face is looked for.
    let inter = bundled(400);
    assert!(!inter.has('\u{416}'));
    let found = face_for(&inter, (400, Style::Normal), '\u{416}');
    assert!(
        found.has('\u{416}') || Arc::ptr_eq(&found, &inter),
        "a face with it, or the chosen one when the machine has none"
    );
    assert!(width_of("\u{416}\u{416}", 0.0) >= 0.0);
}

#[test]
fn moving_and_fading_a_block_should_reuse_the_glyphs_it_drew() {
    let mut a = block("cached");
    let first = draw_block(&bundled(400), &a);
    a.centre = [10.0, 10.0];
    a.rotation = 30.0;
    a.opacity = 0.5;
    assert!(Arc::ptr_eq(&first, &draw_block(&bundled(400), &a)));
    a.size = 41.0;
    assert!(!Arc::ptr_eq(&first, &draw_block(&bundled(400), &a)));
}

#[test]
fn a_colour_tile_should_land_at_its_offset_and_keep_the_pixels_under_it_when_clear() {
    let tile = colour::Tile {
        x: -1,
        y: -1,
        w: 2,
        h: 1,
        rgba: vec![255, 0, 0, 255, 0, 0, 0, 0],
    };
    let mut canvas = vec![0u8; 4 * 3 * 4];
    canvas[(2 * 4 + 1) * 4..][..4].copy_from_slice(&[0, 9, 0, 255]);
    over(&mut canvas, (4, 3), &tile, 2, 2);
    // Tile origin (2, 2) minus (1, 1): the red pixel is at (1, 1), the clear one over (2, 1).
    assert_eq!(&canvas[(4 + 1) * 4..][..4], &[255, 0, 0, 255]);
    assert_eq!(&canvas[(2 * 4 + 1) * 4..][..4], &[0, 9, 0, 255]);
}

#[test]
fn style_names_should_read_back() {
    for (weight, style) in [
        (400, Style::Normal),
        (700, Style::Normal),
        (400, Style::Italic),
        (700, Style::Italic),
        (300, Style::Oblique),
    ] {
        assert_eq!(parse_style(&style_name(weight, style)), (weight, style));
    }
    assert_eq!(style_name(700, Style::Italic), "Bold Italic");
    assert_eq!(parse_style("nonsense"), (400, Style::Normal));
}

/// Columns (first, last) of the ink of `text` in a bitmap as wide as its own cover.
fn ink_columns(block: &Block<'_>) -> (usize, usize, usize) {
    let cover = draw_lines(
        &bundled(if block.style == "Bold" { 700 } else { 400 }),
        block,
    );
    let lit = |x: usize| (0..cover.height).any(|y| cover.bytes[y * cover.width + x] > 0);
    let first = (0..cover.width).find(|&x| lit(x)).unwrap();
    let last = (0..cover.width).rev().find(|&x| lit(x)).unwrap();
    (first, last, cover.width)
}

#[test]
fn ink_should_stay_inside_the_bitmap_for_overhanging_glyphs() {
    for text in ["BEBA", "AVA", "W", "AVATAR", "fj"] {
        for tracking in [0.0, 0.1, -0.05] {
            for style in ["Regular", "Bold"] {
                let size = 300.0;
                let (first, last, width) = ink_columns(&Block {
                    size,
                    tracking,
                    style,
                    ..block(text)
                });
                assert!(
                    first >= 1 && last + 2 <= width,
                    "{text} {style} {tracking}: {first}..{last} of {width}"
                );
            }
        }
    }
}

#[test]
fn drawn_ink_should_match_the_measured_width() {
    // At tracking 0 the line is as wide as its advances: the ink is within a few percent of it.
    let b = Block {
        size: 300.0,
        ..block("BEBA")
    };
    let face = bundled(400);
    let measured = layout_line(&face, parse_style("Regular"), 300.0, "BEBA", 0.0).width;
    let (first, last, _) = ink_columns(&b);
    let ink = (last - first) as f32;
    assert!(
        (ink - measured).abs() < measured * 0.08,
        "ink {ink} vs measured {measured}"
    );
}

#[test]
fn letters_should_not_overlap_at_zero_tracking() {
    let face = bundled(400);
    let line = layout_line(&face, parse_style("Regular"), 300.0, "BEBA", 0.0);
    let boxes: Vec<(f32, f32)> = line
        .glyphs
        .iter()
        .map(|g| {
            let o = face
                .glyphs
                .outline_glyph(g.id.with_scale_and_position(face.scale(300.0), point(g.x, 0.0)))
                .unwrap();
            (o.px_bounds().min.x, o.px_bounds().max.x)
        })
        .collect();
    for pair in boxes.windows(2) {
        assert!(pair[0].1 <= pair[1].0, "{pair:?}");
    }
}
