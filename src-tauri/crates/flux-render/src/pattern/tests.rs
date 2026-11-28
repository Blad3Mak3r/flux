use super::*;
use flux_project::mapping::{CardLayers, Scheme};

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let at = ((y * width + x) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2]]
}

fn slice(card: TestCard) -> CardSlice {
    CardSlice {
        name: "Slice 1".into(),
        input: [0.0, 0.0, 1.0, 1.0],
        card,
        logo: None,
    }
}

/// The card with only the layers named.
fn only(grid: bool, numbers: bool) -> TestCard {
    TestCard {
        layers: CardLayers {
            grid,
            numbers,
            circle: false,
            diagonals: false,
            crosshair: false,
            label: false,
            bars: false,
            motion: false,
        },
        ..TestCard::default()
    }
}

fn made(card: TestCard, comp: (u32, u32)) -> SliceCard {
    layout(&[slice(card)], comp, "").remove(0)
}

#[test]
fn the_grid_lines_should_fall_on_the_first_pixel_of_each_cabinet() {
    let (w, h) = (512, 384);
    let card = made(only(true, false), (w, h));
    let p = palette::palette(palette::default_base(0), Scheme::default());
    for x in [128, 256, 384] {
        assert_eq!(pixel(&card.fixed, w, x, 50), p.line, "column {x}");
        // One pixel wide: both neighbours are cabinet.
        assert_ne!(pixel(&card.fixed, w, x - 1, 50), p.line);
        assert_ne!(pixel(&card.fixed, w, x + 1, 50), p.line);
    }
    for y in [128, 256] {
        assert_eq!(pixel(&card.fixed, w, 60, y), p.line, "row {y}");
        assert_ne!(pixel(&card.fixed, w, 60, y + 1), p.line);
        assert_ne!(pixel(&card.fixed, w, 60, y - 1), p.line);
    }
    // The slice's outline is on its edge.
    assert_eq!(pixel(&card.fixed, w, w - 1, 70), p.frame);
    assert_eq!(pixel(&card.fixed, w, 70, 0), p.frame);
}

#[test]
fn the_cabinet_size_should_move_the_lines() {
    let (w, h) = (256, 256);
    let card = made(
        TestCard {
            cabinet_width: 64,
            cabinet_height: 32,
            ..only(true, false)
        },
        (w, h),
    );
    let p = palette::palette(palette::default_base(0), Scheme::default());
    assert_eq!(pixel(&card.fixed, w, 64, 10), p.line);
    assert_eq!(pixel(&card.fixed, w, 140, 32), p.line);
    assert_ne!(pixel(&card.fixed, w, 100, 40), p.line);
}

#[test]
fn the_checker_should_alternate_per_cabinet_and_the_last_cabinet_should_be_cut() {
    // 300 x 200 with 128 cabinets: a third column of 44 pixels and a second row of 72.
    let (w, h) = (300, 200);
    let card = made(only(false, false), (w, h));
    let p = palette::palette(palette::default_base(0), Scheme::default());
    assert_eq!(pixel(&card.fixed, w, 10, 10), p.cabinet[0]);
    assert_eq!(pixel(&card.fixed, w, 130, 10), p.cabinet[1]);
    assert_eq!(pixel(&card.fixed, w, 10, 130), p.cabinet[1]);
    assert_eq!(pixel(&card.fixed, w, 290, 190), p.cabinet[1]);
}

#[test]
fn ids_should_follow_the_numbering() {
    assert_eq!(cabinet_id(Numbering::Alpha, 0, 0, 12), "A1");
    assert_eq!(cabinet_id(Numbering::Alpha, 1, 0, 12), "B1");
    assert_eq!(cabinet_id(Numbering::Alpha, 26, 2, 30), "AA3");
    assert_eq!(cabinet_id(Numbering::Dotted, 2, 4, 12), "3.5");
    assert_eq!(cabinet_id(Numbering::Sequence, 2, 1, 12), "15");
    assert_eq!(cabinet_id(Numbering::Sequence, 0, 0, 12), "01");
}

#[test]
fn numbers_should_be_written_in_the_cabinets_that_are_big_enough_and_not_in_small_ones() {
    let (w, h) = (256, 128);
    let with = made(only(false, true), (w, h));
    let without = made(only(false, false), (w, h));
    // The top left of each of the two cabinets differs, the middle of one does not.
    let differs = |x0: u32| {
        (x0 + 4..x0 + 40)
            .any(|x| (4..30).any(|y| pixel(&with.fixed, w, x, y) != pixel(&without.fixed, w, x, y)))
    };
    assert!(differs(0) && differs(128));
    assert_eq!(
        pixel(&with.fixed, w, 100, 110),
        pixel(&without.fixed, w, 100, 110)
    );
    // Cabinets of 32 pixels are too small to read: no ids.
    let small = TestCard {
        cabinet_width: 32,
        cabinet_height: 32,
        ..only(false, true)
    };
    let small_without = TestCard {
        cabinet_width: 32,
        cabinet_height: 32,
        ..only(false, false)
    };
    assert_eq!(made(small, (w, h)).fixed, made(small_without, (w, h)).fixed);
}

#[test]
fn a_solid_mode_should_fill_every_pixel() {
    let (w, h) = (96, 64);
    for (mode, colour) in [
        (CardMode::Red, [255, 0, 0]),
        (CardMode::Green, [0, 255, 0]),
        (CardMode::Blue, [0, 0, 255]),
        (CardMode::White, [255, 255, 255]),
        (CardMode::Black, [0, 0, 0]),
    ] {
        let card = made(
            TestCard {
                mode,
                ..TestCard::default()
            },
            (w, h),
        );
        assert!(
            card.fixed
                .chunks(4)
                .all(|px| px == [colour[0], colour[1], colour[2], 255])
        );
        assert!(
            !card.frame(&mut Vec::new(), 1.0, "00:00:00"),
            "no motion on a flat colour"
        );
    }
    let custom = TestCard {
        mode: CardMode::Custom,
        custom: "#102030".into(),
        ..TestCard::default()
    };
    assert_eq!(
        pixel(&made(custom, (w, h)).fixed, w, 5, 5),
        [0x10, 0x20, 0x30]
    );
}

#[test]
fn the_card_should_be_deterministic_and_have_the_size_of_its_slice() {
    let slices = [CardSlice {
        name: "Wall".into(),
        input: [0.25, 0.5, 0.5, 0.25],
        card: TestCard::default(),
        logo: None,
    }];
    let a = layout(&slices, (1920, 1080), "Main  1920 x 1080");
    let b = layout(&slices, (1920, 1080), "Main  1920 x 1080");
    assert_eq!(a[0].rect, [480, 540, 960, 270]);
    assert_eq!(a[0].fixed.len(), 960 * 270 * 4);
    assert_eq!(a[0].fixed, b[0].fixed);
    let (mut one, mut two) = (Vec::new(), Vec::new());
    assert!(a[0].frame(&mut one, 1.25, "10:20:30"));
    assert!(b[0].frame(&mut two, 1.25, "10:20:30"));
    assert_eq!(one, two);
    // It moves: a quarter of a second later it is not the same picture.
    assert!(a[0].frame(&mut two, 1.5, "10:20:30"));
    assert_ne!(one, two);
}

#[test]
fn the_label_should_be_drawn_in_the_middle_of_the_slice() {
    let (w, h) = (640, 384);
    let plain = made(only(false, false), (w, h));
    let labelled = made(
        TestCard {
            layers: CardLayers {
                label: true,
                ..only(false, false).layers
            },
            ..only(false, false)
        },
        (w, h),
    );
    let changed = |x: u32, y: u32| pixel(&plain.fixed, w, x, y) != pixel(&labelled.fixed, w, x, y);
    assert!(
        changed(w / 2, h / 2)
            || (w / 2 - 60..w / 2 + 60).any(|x| (h / 2 - 40..h / 2 + 40).any(|y| changed(x, y)))
    );
    assert!(!changed(5, 5) && !changed(w - 6, h - 6));
}

#[test]
fn a_partial_slice_pixel_rect_should_snap_to_whole_pixels() {
    assert_eq!(
        pixel_rect([0.0, 0.0, 1.0, 1.0], (1920, 1080)),
        [0, 0, 1920, 1080]
    );
    assert_eq!(
        pixel_rect([0.3333, 0.0, 0.3333, 1.0], (1920, 1080)),
        [640, 0, 640, 1080]
    );
    let tiny = pixel_rect([0.5, 0.5, 0.0001, 0.0001], (100, 100));
    assert!(tiny[2] >= 1 && tiny[3] >= 1);
}

#[test]
fn the_preview_should_be_clear_outside_the_cards_and_keep_their_lines() {
    let slices = [CardSlice {
        name: "A".into(),
        input: [0.0, 0.0, 0.5, 1.0],
        card: only(true, false),
        logo: None,
    }];
    let cards = layout(&slices, (512, 256), "");
    let (w, h, rgba) = preview(&cards, (512, 256), 256);
    assert_eq!((w, h), (256, 128));
    assert_eq!(rgba[((10 * w + 200) * 4 + 3) as usize], 0);
    assert_eq!(rgba[((10 * w + 20) * 4 + 3) as usize], 255);
}

/// A red square logo, 32 by 32.
fn red_logo() -> Logo {
    Logo {
        id: "img".into(),
        width: 32,
        height: 32,
        rgba: [255u8, 0, 0, 255].repeat(32 * 32).into(),
    }
}

fn with_logo(show: bool) -> SliceCard {
    let card = TestCard {
        show_logo: show,
        logo_back: "#000000".into(),
        layers: CardLayers {
            label: false,
            bars: false,
            motion: false,
            ..CardLayers::default()
        },
        ..TestCard::default()
    };
    let slices = [CardSlice {
        logo: Some(red_logo()),
        ..slice(card)
    }];
    layout(&slices, (640, 480), "").remove(0)
}

#[test]
fn the_logo_should_be_drawn_in_a_disc_when_it_is_shown() {
    let card = with_logo(true);
    let found = (0..480u32).any(|y| pixel(&card.fixed, 640, 320, y) == [255, 0, 0]);
    assert!(found);
}

#[test]
fn the_logo_should_be_left_out_when_it_is_off() {
    let card = with_logo(false);
    let found = (0..480u32).any(|y| pixel(&card.fixed, 640, 320, y) == [255, 0, 0]);
    assert!(!found);
}

#[test]
fn the_logo_should_sit_on_the_back_colour_of_its_disc() {
    let card = with_logo(true);
    let back = (0..480u32)
        .filter(|&y| pixel(&card.fixed, 640, 320, y) == [0, 0, 0])
        .count();
    assert!(back > 0);
}

#[test]
fn the_moving_part_should_stay_in_the_stack_and_not_cross_the_whole_slice() {
    let card = made(TestCard::default(), (1280, 720));
    let (mut a, mut b) = (Vec::new(), Vec::new());
    assert!(card.frame(&mut a, 0.2, "10:20:30"));
    assert!(card.frame(&mut b, 1.4, "10:20:30"));
    let rows: Vec<u32> = (0..720u32)
        .filter(|&y| {
            let at = (y * 1280 * 4) as usize;
            let row = 1280 * 4;
            a[at..at + row] != b[at..at + row]
        })
        .collect();
    let (first, last) = (rows[0], rows[rows.len() - 1]);
    assert!(last - first < 160, "rows {first} to {last}");
}

#[test]
fn the_ids_should_be_drawn_over_the_guides() {
    let layers = CardLayers {
        grid: false,
        circle: false,
        diagonals: true,
        crosshair: false,
        label: false,
        bars: false,
        motion: false,
        ..CardLayers::default()
    };
    let with = |numbers| TestCard {
        layers: CardLayers { numbers, ..layers },
        guide_width: 12,
        ..TestCard::default()
    };
    let (a, b) = (made(with(true), (512, 512)), made(with(false), (512, 512)));
    let differ = (0..60u32)
        .flat_map(|y| (0..90u32).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(&a.fixed, 512, x, y) != pixel(&b.fixed, 512, x, y))
        .count();
    assert!(differ > 40, "{differ}");
}
