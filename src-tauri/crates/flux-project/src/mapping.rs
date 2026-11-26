//! How the video reaches the screen, like the advanced output of a media server: the picture of
//! the timeline (the composition) is cut into slices, and each slice is a rectangle of it placed on
//! the output by its four corners, which can lean it to fit a surface seen at an angle. No slices
//! is the plain output: the whole picture fitted to the screen.
//!
//! A slice can fade to black towards its edges (a soft edge), so two projectors whose pictures
//! overlap blend into one; a screen's masks black out shapes of it, or everything but them.
//!
//! The output has one or more screens, each a window on a display of its own with its own slices.
//! The main screen always exists, even when the project does not list it; which display each
//! screen opens on is a preference of the computer, not of the project.
//!
//! Every number is a fraction of a width or height, from the top left corner, so a mapping keeps
//! its shape on a screen of another resolution.

use serde::{Deserialize, Serialize};

use crate::{ProjectError, Result};

/// Most slices a screen has.
pub const MAX_SLICES: usize = 64;
/// Most screens an output has.
pub const MAX_SCREENS: usize = 8;
/// Most masks a screen has, and most points of one.
pub const MAX_MASKS: usize = 8;
pub const MAX_MASK_POINTS: usize = 32;
/// The id of the screen every project has.
pub const MAIN_SCREEN: &str = "main";
/// Longest slice name, in characters.
const MAX_NAME: usize = 80;
/// Smallest side of a slice's rectangle, so it never vanishes.
const MIN_SIDE: f32 = 0.005;
/// How far a corner may go past the screen's edges: a projector's picture can hang off them.
const CORNER_REACH: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const FULL: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };
}

/// The corners of the whole screen: top left, top right, bottom right, bottom left.
pub const FULL_CORNERS: [Point; 4] = [
    Point { x: 0.0, y: 0.0 },
    Point { x: 1.0, y: 0.0 },
    Point { x: 1.0, y: 1.0 },
    Point { x: 0.0, y: 1.0 },
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slice {
    pub id: String,
    pub name: String,
    /// The part of the composition it shows.
    pub input: Rect,
    /// Where that part's top left, top right, bottom right and bottom left corners go on the
    /// output.
    pub corners: [Point; 4],
    /// Left out of the output without losing its place.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "SoftEdge::is_hard")]
    pub soft_edge: SoftEdge,
    /// How the output's test card is drawn on this slice.
    #[serde(default)]
    pub test_card: TestCard,
}

/// Cabinet sides a test card accepts, in pixels.
pub const CABINET_SIDES: std::ops::RangeInclusive<u32> = 8..=2048;

/// How the cabinets of the grid are numbered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Numbering {
    /// Column letter then row number, as a spreadsheet: A1, B1, A2.
    #[default]
    Alpha,
    /// Column then row: 1.1, 2.1, 1.2.
    Dotted,
    /// Row by row from the top left: 01, 02, 03.
    Sequence,
}

/// The card's palette.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scheme {
    /// Two saturated tones of the base colour, with white guides.
    #[default]
    Vivid,
    /// Near black cabinets tinted by the base colour, with a faint checker.
    Dark,
    /// Pale cabinets, for a wall seen in daylight.
    Light,
    /// Pure black and white cabinets, as strong as a checker gets.
    Contrast,
}

/// What a card fills the slice with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CardMode {
    /// The cabinet grid with its layers.
    #[default]
    Pattern,
    Red,
    Green,
    Blue,
    White,
    Black,
    /// The card's `custom` colour, for dead pixel and uniformity checks.
    Custom,
}

/// Which parts of the pattern are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CardLayers {
    /// One pixel lines on the cabinet boundaries.
    pub grid: bool,
    /// The cabinet ids.
    pub numbers: bool,
    pub circle: bool,
    pub diagonals: bool,
    pub crosshair: bool,
    /// The label: the slice's name, resolution and cabinet count.
    pub label: bool,
    /// Colour bars and a grey ramp.
    pub bars: bool,
    /// A sweeping bar, a clock and a frame counter, which show a frozen or stuttering output.
    pub motion: bool,
}

impl Default for CardLayers {
    fn default() -> Self {
        Self {
            grid: true,
            numbers: true,
            circle: true,
            diagonals: true,
            crosshair: true,
            label: true,
            bars: true,
            motion: true,
        }
    }
}

/// The test card of a slice, drawn in the slice's own pixels so the cabinet lines fall where the
/// physical cabinets meet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TestCard {
    /// Shows the card on this slice even while the output's card is off.
    pub show: bool,
    /// The physical cabinet's size in pixels.
    pub cabinet_width: u32,
    pub cabinet_height: u32,
    pub numbering: Numbering,
    pub scheme: Scheme,
    /// How thick the guides (circle, diagonals, centre lines) are, in pixels.
    pub guide_width: u32,
    /// The label shows the slice's name.
    pub show_name: bool,
    /// The label shows the slice's resolution.
    pub show_resolution: bool,
    /// The label has a dark rounded plate under it.
    pub label_plate: bool,
    /// Where the label sits, 0 (top) to 100 (bottom) of the slice's height.
    pub label_y: u32,
    /// `#rrggbb`, the colour the card's whole palette is made from; empty takes a hue from the slice's place in the list.
    pub base: String,
    pub layers: CardLayers,
    pub mode: CardMode,
    /// `#rrggbb`, the fill of [`CardMode::Custom`].
    pub custom: String,
    /// Draws the logo in a disc above the label.
    pub show_logo: bool,
    /// The id of the project's image used as the logo; empty for none.
    pub logo: String,
    /// `#rrggbb`, the disc behind the logo.
    pub logo_back: String,
    /// The disc's diameter, as a percentage of the slice's short side.
    pub logo_size: u32,
}

/// The logo's size, in percent of the slice's short side.
pub const LOGO_SIZES: std::ops::RangeInclusive<u32> = 10..=60;

impl Default for TestCard {
    fn default() -> Self {
        Self {
            show: false,
            cabinet_width: 128,
            cabinet_height: 128,
            numbering: Numbering::default(),
            scheme: Scheme::default(),
            guide_width: 2,
            show_name: true,
            show_resolution: true,
            label_plate: true,
            label_y: 50,
            base: String::new(),
            layers: CardLayers::default(),
            mode: CardMode::default(),
            custom: "#ff00ff".into(),
            show_logo: false,
            logo: String::new(),
            logo_back: "#10131a".into(),
            logo_size: 24,
        }
    }
}

impl TestCard {
    /// The base colour, `None` when it is left to the slice's place.
    pub fn base_rgb(&self) -> Option<[u8; 3]> {
        parse_hex(&self.base)
    }

    pub fn custom_rgb(&self) -> [u8; 3] {
        parse_hex(&self.custom).unwrap_or([255, 0, 255])
    }

    pub fn logo_back_rgb(&self) -> [u8; 3] {
        parse_hex(&self.logo_back).unwrap_or([16, 19, 26])
    }

    fn validate(&self) -> Result<()> {
        let invalid = |why: &str| Err(ProjectError::Invalid(format!("video test card: {why}")));
        if !(1..=12).contains(&self.guide_width) || self.label_y > 100 {
            return invalid("the guide width is 1 to 12 pixels and the label's place 0 to 100");
        }
        if !CABINET_SIDES.contains(&self.cabinet_width)
            || !CABINET_SIDES.contains(&self.cabinet_height)
        {
            return invalid("a cabinet side is from 8 to 2048 pixels");
        }
        if !self.base.is_empty() && parse_hex(&self.base).is_none() {
            return invalid("the base colour is #rrggbb");
        }
        if parse_hex(&self.custom).is_none() || parse_hex(&self.logo_back).is_none() {
            return invalid("the colour is #rrggbb");
        }
        Ok(())
    }
}

/// `#rrggbb` as its three bytes.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let byte = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// How far in from each edge a slice fades to black, each a fraction (0 to 1) of its width or
/// height, and the fade's curve: 1 fades the light evenly, more keeps it brighter for longer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SoftEdge {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    pub gamma: f32,
}

impl Default for SoftEdge {
    fn default() -> Self {
        Self {
            left: 0.0,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
            gamma: 1.0,
        }
    }
}

impl SoftEdge {
    pub fn is_hard(&self) -> bool {
        *self == Self::default()
    }
}

/// The curve's range: from a quick fade to a slow one.
const GAMMA: std::ops::RangeInclusive<f32> = 0.2..=5.0;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EdgeField {
    Left,
    Right,
    Top,
    Bottom,
    Gamma,
}

/// A shape on a screen that hides the picture inside it, or outside it when `invert`: a pillar,
/// a door, or everything around an odd-shaped surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mask {
    pub id: String,
    pub name: String,
    /// The outline, in order, as fractions of the screen from its top left corner.
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MaskOp {
    /// A square in the middle of the screen, named "Mask n".
    Add,
    Remove {
        id: String,
    },
    Rename {
        id: String,
        name: String,
    },
    /// Moves the points; there must be as many as before.
    SetPoints {
        id: String,
        points: Vec<Point>,
    },
    /// A point halfway between point `after` and the next one.
    AddPoint {
        id: String,
        after: usize,
    },
    /// A mask keeps at least three points.
    RemovePoint {
        id: String,
        index: usize,
    },
    SetInvert {
        id: String,
        invert: bool,
    },
    SetHidden {
        id: String,
        hidden: bool,
    },
}

/// Applies `op` to a screen's `masks`; `new_id` names a new mask. The caller validates the result.
pub fn edit_masks(
    masks: &mut Vec<Mask>,
    op: MaskOp,
    new_id: impl FnOnce() -> String,
) -> Result<()> {
    let find = |masks: &mut Vec<Mask>, id: &str| -> Result<usize> {
        masks
            .iter()
            .position(|mask| mask.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no mask {id}")))
    };
    let inside = |p: Point| Point {
        x: finite_or(p.x, 0.5).clamp(0.0, 1.0),
        y: finite_or(p.y, 0.5).clamp(0.0, 1.0),
    };
    match op {
        MaskOp::Add => {
            if masks.len() >= MAX_MASKS {
                return Err(ProjectError::Invalid("too many masks".into()));
            }
            let number = (1..)
                .find(|n| !masks.iter().any(|m| m.name == format!("Mask {n}")))
                .unwrap_or(1);
            let (a, b) = (0.4, 0.6);
            masks.push(Mask {
                id: new_id(),
                name: format!("Mask {number}"),
                points: vec![
                    Point { x: a, y: a },
                    Point { x: b, y: a },
                    Point { x: b, y: b },
                    Point { x: a, y: b },
                ],
                invert: false,
                hidden: false,
            });
        }
        MaskOp::Remove { id } => {
            let at = find(masks, &id)?;
            masks.remove(at);
        }
        MaskOp::Rename { id, name } => {
            let at = find(masks, &id)?;
            masks[at].name = name.trim().to_owned();
        }
        MaskOp::SetPoints { id, points } => {
            let at = find(masks, &id)?;
            if points.len() != masks[at].points.len() {
                return Err(ProjectError::Invalid(
                    "a mask's points cannot be counted anew".into(),
                ));
            }
            masks[at].points = points.into_iter().map(inside).collect();
        }
        MaskOp::AddPoint { id, after } => {
            let at = find(masks, &id)?;
            let points = &mut masks[at].points;
            if points.len() >= MAX_MASK_POINTS || after >= points.len() {
                return Err(ProjectError::Invalid("no room for another point".into()));
            }
            let (a, b) = (points[after], points[(after + 1) % points.len()]);
            let middle = Point {
                x: (a.x + b.x) / 2.0,
                y: (a.y + b.y) / 2.0,
            };
            points.insert(after + 1, middle);
        }
        MaskOp::RemovePoint { id, index } => {
            let at = find(masks, &id)?;
            let points = &mut masks[at].points;
            if points.len() <= 3 || index >= points.len() {
                return Err(ProjectError::Invalid("a mask keeps three points".into()));
            }
            points.remove(index);
        }
        MaskOp::SetInvert { id, invert } => {
            let at = find(masks, &id)?;
            masks[at].invert = invert;
        }
        MaskOp::SetHidden { id, hidden } => {
            let at = find(masks, &id)?;
            masks[at].hidden = hidden;
        }
    }
    Ok(())
}

fn finite_or(v: f32, or: f32) -> f32 {
    if v.is_finite() { v } else { or }
}

/// A window of the output, on a display of its own, and the slices drawn on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoScreen {
    pub id: String,
    pub name: String,
    /// None is the whole picture fitted to the screen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slices: Vec<Slice>,
    /// Drawn over the slices, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub masks: Vec<Mask>,
}

impl VideoScreen {
    fn main() -> Self {
        Self {
            id: MAIN_SCREEN.into(),
            name: "Screen 1".into(),
            slices: Vec::new(),
            masks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ScreenOp {
    /// A new screen with no slices, named "Screen n".
    Add,
    /// Any screen but the main one.
    Remove {
        id: String,
    },
    Rename {
        id: String,
        name: String,
    },
}

/// The screens of an output with the main one first, as `screens` lists them or made when it does
/// not.
pub fn screens(screens: &[VideoScreen]) -> Vec<VideoScreen> {
    let mut all = screens.to_vec();
    with_main(&mut all);
    all
}

fn with_main(screens: &mut Vec<VideoScreen>) {
    if !screens.iter().any(|screen| screen.id == MAIN_SCREEN) {
        screens.insert(0, VideoScreen::main());
    }
}

/// The screen `id` of `screens`, the main one added first when it is missing.
pub fn screen_mut<'a>(screens: &'a mut Vec<VideoScreen>, id: &str) -> Result<&'a mut VideoScreen> {
    with_main(screens);
    screens
        .iter_mut()
        .find(|screen| screen.id == id)
        .ok_or_else(|| ProjectError::Invalid(format!("no screen {id}")))
}

/// Applies `op` to `screens`; `new_id` names a new screen. The caller validates the result.
pub fn edit_screens(
    screens: &mut Vec<VideoScreen>,
    op: ScreenOp,
    new_id: impl FnOnce() -> String,
) -> Result<()> {
    with_main(screens);
    match op {
        ScreenOp::Add => {
            if screens.len() >= MAX_SCREENS {
                return Err(ProjectError::Invalid("too many screens".into()));
            }
            let number = (1..)
                .find(|n| !screens.iter().any(|s| s.name == format!("Screen {n}")))
                .unwrap_or(1);
            screens.push(VideoScreen {
                id: new_id(),
                name: format!("Screen {number}"),
                slices: Vec::new(),
                masks: Vec::new(),
            });
        }
        ScreenOp::Remove { id } => {
            if id == MAIN_SCREEN {
                return Err(ProjectError::Invalid("the main screen stays".into()));
            }
            let before = screens.len();
            screens.retain(|screen| screen.id != id);
            if screens.len() == before {
                return Err(ProjectError::Invalid(format!("no screen {id}")));
            }
        }
        ScreenOp::Rename { id, name } => {
            screen_mut(screens, &id)?.name = name.trim().to_owned();
        }
    }
    Ok(())
}

pub fn validate_screens(screens: &[VideoScreen]) -> Result<()> {
    let invalid = |why: &str| Err(ProjectError::Invalid(format!("video screens: {why}")));
    if screens.len() > MAX_SCREENS {
        return invalid("too many screens");
    }
    let mut ids = std::collections::HashSet::new();
    for screen in screens {
        if screen.id.is_empty() || !ids.insert(screen.id.as_str()) {
            return invalid("a screen id is empty or repeated");
        }
        if screen.name.is_empty() || screen.name.chars().count() > MAX_NAME {
            return invalid("a screen name is empty or too long");
        }
        validate(&screen.slices)?;
        validate_masks(&screen.masks)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MappingOp {
    /// A new slice of the whole composition on the whole screen, named "Slice n".
    Add,
    Remove {
        id: String,
    },
    Rename {
        id: String,
        name: String,
    },
    SetInput {
        id: String,
        input: Rect,
    },
    SetCorners {
        id: String,
        corners: [Point; 4],
    },
    /// Sets one side of the rectangle to a typed number of pixels of a picture of `size`.
    SetInputValue {
        id: String,
        field: RectField,
        text: String,
        size: Size,
    },
    /// Sets one coordinate of a corner to a typed number of pixels of a screen of `size`.
    SetCornerValue {
        id: String,
        corner: usize,
        axis: Axis,
        text: String,
        size: Size,
    },
    /// Sets one side of the soft edge to a typed percentage of the slice, or its curve.
    SetSoftEdge {
        id: String,
        field: EdgeField,
        text: String,
    },
    /// Puts the slice's corners back on the corners of its rectangle, as the screen's.
    ResetCorners {
        id: String,
    },
    SetHidden {
        id: String,
        hidden: bool,
    },
    /// Replaces the slice's test card settings; the sides are kept to the cabinet range.
    SetTestCard {
        id: String,
        card: TestCard,
    },
    /// Gives every slice of the screen this slice's test card settings, each keeping its own
    /// colour and whether it shows the card.
    CopyTestCard {
        id: String,
    },
    /// Takes every slice away: the output shows the whole picture again.
    Clear,
}

/// A picture's or a screen's size in pixels, to turn typed pixels into fractions.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RectField {
    X,
    Y,
    Width,
    Height,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Axis {
    X,
    Y,
}

/// A typed number of pixels as a fraction of `of` pixels.
fn fraction(text: &str, of: f32) -> Result<f32> {
    let pixels = text
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| ProjectError::Invalid("not a number of pixels".into()))?;
    if !(of.is_finite() && of >= 1.0) {
        return Err(ProjectError::Invalid("no size to measure pixels in".into()));
    }
    Ok(pixels / of)
}

/// Applies `op` to `slices`; `new_id` names a new slice. The caller validates the result.
pub fn apply(
    slices: &mut Vec<Slice>,
    op: MappingOp,
    new_id: impl FnOnce() -> String,
) -> Result<()> {
    let find = |slices: &mut Vec<Slice>, id: &str| -> Result<usize> {
        slices
            .iter()
            .position(|slice| slice.id == id)
            .ok_or_else(|| ProjectError::Invalid(format!("no slice {id}")))
    };
    match op {
        MappingOp::Add => {
            let number = (1..)
                .find(|n| !slices.iter().any(|s| s.name == format!("Slice {n}")))
                .unwrap_or(1);
            slices.push(Slice {
                id: new_id(),
                name: format!("Slice {number}"),
                input: Rect::FULL,
                corners: FULL_CORNERS,
                hidden: false,
                soft_edge: SoftEdge::default(),
                test_card: TestCard::default(),
            });
        }
        MappingOp::Remove { id } => {
            let at = find(slices, &id)?;
            slices.remove(at);
        }
        MappingOp::Rename { id, name } => {
            let at = find(slices, &id)?;
            slices[at].name = name.trim().to_owned();
        }
        MappingOp::SetInput { id, input } => {
            let at = find(slices, &id)?;
            slices[at].input = clamp_rect(input);
        }
        MappingOp::SetCorners { id, corners } => {
            let at = find(slices, &id)?;
            slices[at].corners = corners.map(|p| Point {
                x: p.x.clamp(-CORNER_REACH, 1.0 + CORNER_REACH),
                y: p.y.clamp(-CORNER_REACH, 1.0 + CORNER_REACH),
            });
        }
        MappingOp::SetInputValue {
            id,
            field,
            text,
            size,
        } => {
            let at = find(slices, &id)?;
            let mut r = slices[at].input;
            // A side that would run past the picture is shortened, and a position moves the
            // rectangle without changing its size, as far as the picture allows.
            match field {
                RectField::X => r.x = fraction(&text, size.width)?.clamp(0.0, 1.0 - r.width),
                RectField::Y => r.y = fraction(&text, size.height)?.clamp(0.0, 1.0 - r.height),
                RectField::Width => {
                    r.width = fraction(&text, size.width)?.clamp(MIN_SIDE, 1.0 - r.x)
                }
                RectField::Height => {
                    r.height = fraction(&text, size.height)?.clamp(MIN_SIDE, 1.0 - r.y)
                }
            }
            slices[at].input = clamp_rect(r);
        }
        MappingOp::SetCornerValue {
            id,
            corner,
            axis,
            text,
            size,
        } => {
            let at = find(slices, &id)?;
            let point = slices[at]
                .corners
                .get_mut(corner)
                .ok_or_else(|| ProjectError::Invalid(format!("no corner {corner}")))?;
            let reach = -CORNER_REACH..=1.0 + CORNER_REACH;
            match axis {
                Axis::X => {
                    point.x = fraction(&text, size.width)?.clamp(*reach.start(), *reach.end())
                }
                Axis::Y => {
                    point.y = fraction(&text, size.height)?.clamp(*reach.start(), *reach.end())
                }
            }
        }
        MappingOp::SetSoftEdge { id, field, text } => {
            let at = find(slices, &id)?;
            let value = text
                .trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .ok_or_else(|| ProjectError::Invalid("not a number".into()))?;
            let percent = |v: f32| (v / 100.0).clamp(0.0, 1.0);
            let edge = &mut slices[at].soft_edge;
            match field {
                EdgeField::Left => edge.left = percent(value),
                EdgeField::Right => edge.right = percent(value),
                EdgeField::Top => edge.top = percent(value),
                EdgeField::Bottom => edge.bottom = percent(value),
                EdgeField::Gamma => edge.gamma = value.clamp(*GAMMA.start(), *GAMMA.end()),
            }
        }
        MappingOp::ResetCorners { id } => {
            let at = find(slices, &id)?;
            let Rect {
                x,
                y,
                width,
                height,
            } = slices[at].input;
            slices[at].corners = [
                Point { x, y },
                Point { x: x + width, y },
                Point {
                    x: x + width,
                    y: y + height,
                },
                Point { x, y: y + height },
            ];
        }
        MappingOp::SetHidden { id, hidden } => {
            let at = find(slices, &id)?;
            slices[at].hidden = hidden;
        }
        MappingOp::SetTestCard { id, mut card } => {
            let at = find(slices, &id)?;
            let (low, high) = (*CABINET_SIDES.start(), *CABINET_SIDES.end());
            card.cabinet_width = card.cabinet_width.clamp(low, high);
            card.cabinet_height = card.cabinet_height.clamp(low, high);
            card.guide_width = card.guide_width.clamp(1, 12);
            card.label_y = card.label_y.min(100);
            card.logo_size = card.logo_size.clamp(*LOGO_SIZES.start(), *LOGO_SIZES.end());
            card.validate()?;
            slices[at].test_card = card;
        }
        MappingOp::CopyTestCard { id } => {
            let at = find(slices, &id)?;
            let from = slices[at].test_card.clone();
            // Every setting but the colour and whether the slice shows it on its own.
            for slice in slices.iter_mut() {
                slice.test_card = TestCard {
                    base: std::mem::take(&mut slice.test_card.base),
                    show: slice.test_card.show,
                    ..from.clone()
                };
            }
        }
        MappingOp::Clear => slices.clear(),
    }
    Ok(())
}

/// Keeps a rectangle inside the composition and at least [`MIN_SIDE`] on each side.
fn clamp_rect(rect: Rect) -> Rect {
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    let width = finite(rect.width, 1.0).clamp(MIN_SIDE, 1.0);
    let height = finite(rect.height, 1.0).clamp(MIN_SIDE, 1.0);
    Rect {
        x: finite(rect.x, 0.0).clamp(0.0, 1.0 - width),
        y: finite(rect.y, 0.0).clamp(0.0, 1.0 - height),
        width,
        height,
    }
}

fn validate_masks(masks: &[Mask]) -> Result<()> {
    let invalid = |why: &str| Err(ProjectError::Invalid(format!("video masks: {why}")));
    if masks.len() > MAX_MASKS {
        return invalid("too many masks");
    }
    let mut ids = std::collections::HashSet::new();
    for mask in masks {
        if mask.id.is_empty() || !ids.insert(mask.id.as_str()) {
            return invalid("a mask id is empty or repeated");
        }
        if mask.name.is_empty() || mask.name.chars().count() > MAX_NAME {
            return invalid("a mask name is empty or too long");
        }
        let on_screen = |v: f32| (0.0..=1.0).contains(&v);
        if !(3..=MAX_MASK_POINTS).contains(&mask.points.len())
            || !mask.points.iter().all(|p| on_screen(p.x) && on_screen(p.y))
        {
            return invalid("a mask needs 3 to 32 points on the screen");
        }
    }
    Ok(())
}

pub fn validate(slices: &[Slice]) -> Result<()> {
    let invalid = |why: &str| Err(ProjectError::Invalid(format!("video mapping: {why}")));
    if slices.len() > MAX_SLICES {
        return invalid("too many slices");
    }
    let mut ids = std::collections::HashSet::new();
    for slice in slices {
        if slice.id.is_empty() || !ids.insert(slice.id.as_str()) {
            return invalid("a slice id is empty or repeated");
        }
        if slice.name.is_empty() || slice.name.chars().count() > MAX_NAME {
            return invalid("a slice name is empty or too long");
        }
        let r = slice.input;
        let inside = |v: f32| (0.0..=1.0).contains(&v);
        if !(inside(r.x)
            && inside(r.y)
            && r.width >= MIN_SIDE
            && r.height >= MIN_SIDE
            && r.x + r.width <= 1.0 + f32::EPSILON
            && r.y + r.height <= 1.0 + f32::EPSILON)
        {
            return invalid("a slice's rectangle is outside the picture");
        }
        let reach = -CORNER_REACH..=1.0 + CORNER_REACH;
        if !slice
            .corners
            .iter()
            .all(|p| reach.contains(&p.x) && reach.contains(&p.y))
        {
            return invalid("a slice's corner is too far off the screen");
        }
        let e = slice.soft_edge;
        let fraction = |v: f32| (0.0..=1.0).contains(&v);
        if !(fraction(e.left)
            && fraction(e.right)
            && fraction(e.top)
            && fraction(e.bottom)
            && GAMMA.contains(&e.gamma))
        {
            return invalid("a slice's soft edge is out of range");
        }
        slice.test_card.validate()?;
    }
    Ok(())
}

/// The size of the composition, the picture every clip is sized into and the slices cut, in
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Composition {
    pub width: u32,
    pub height: u32,
}

/// Sides a composition may have, in pixels.
pub const COMPOSITION_SIDES: std::ops::RangeInclusive<u32> = 16..=8192;

impl Composition {
    /// The size typed as two numbers of pixels.
    pub fn parse(width: &str, height: &str) -> Result<Self> {
        let side = |text: &str| {
            text.trim()
                .parse::<u32>()
                .ok()
                .filter(|v| COMPOSITION_SIDES.contains(v))
                .ok_or_else(|| {
                    ProjectError::Invalid(format!(
                        "the composition's sides are whole pixels from {} to {}",
                        COMPOSITION_SIDES.start(),
                        COMPOSITION_SIDES.end()
                    ))
                })
        };
        Ok(Self {
            width: side(width)?,
            height: side(height)?,
        })
    }

    pub fn validate(&self) -> Result<()> {
        Self::parse(&self.width.to_string(), &self.height.to_string()).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_composition_should_take_whole_pixels_within_its_range() {
        assert_eq!(
            Composition::parse(" 3840", "1080 ").unwrap(),
            Composition {
                width: 3840,
                height: 1080
            }
        );
        for (w, h) in [
            ("15", "1080"),
            ("1920", "8193"),
            ("19.5", "1080"),
            ("", "1080"),
        ] {
            assert!(Composition::parse(w, h).is_err(), "{w} x {h}");
        }
    }

    fn added() -> Vec<Slice> {
        let mut slices = Vec::new();
        apply(&mut slices, MappingOp::Add, || "s1".into()).unwrap();
        slices
    }

    #[test]
    fn add_should_show_the_whole_picture_on_the_whole_screen() {
        let slices = added();
        assert_eq!(slices[0].name, "Slice 1");
        assert_eq!(
            (slices[0].input, slices[0].corners),
            (Rect::FULL, FULL_CORNERS)
        );
        let mut more = slices.clone();
        apply(&mut more, MappingOp::Add, || "s2".into()).unwrap();
        assert_eq!(more[1].name, "Slice 2");
        assert!(validate(&more).is_ok());
    }

    #[test]
    fn set_input_should_keep_the_rectangle_inside_the_picture() {
        let mut slices = added();
        let input = Rect {
            x: 0.8,
            y: -0.2,
            width: 0.5,
            height: 0.0,
        };
        apply(
            &mut slices,
            MappingOp::SetInput {
                id: "s1".into(),
                input,
            },
            String::new,
        )
        .unwrap();
        let r = slices[0].input;
        assert_eq!((r.x, r.y, r.width, r.height), (0.5, 0.0, 0.5, MIN_SIDE));
        assert!(validate(&slices).is_ok());
    }

    #[test]
    fn reset_corners_should_put_the_slice_where_its_rectangle_is() {
        let mut slices = added();
        let input = Rect {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        apply(
            &mut slices,
            MappingOp::SetInput {
                id: "s1".into(),
                input,
            },
            String::new,
        )
        .unwrap();
        apply(
            &mut slices,
            MappingOp::ResetCorners { id: "s1".into() },
            String::new,
        )
        .unwrap();
        assert_eq!(slices[0].corners[0], Point { x: 0.5, y: 0.0 });
        assert_eq!(slices[0].corners[2], Point { x: 1.0, y: 1.0 });
    }

    #[test]
    fn corners_should_stop_a_screen_away_from_the_edges() {
        let mut slices = added();
        let far = [Point { x: -9.0, y: 0.0 }; 4];
        apply(
            &mut slices,
            MappingOp::SetCorners {
                id: "s1".into(),
                corners: far,
            },
            String::new,
        )
        .unwrap();
        assert_eq!(slices[0].corners[0].x, -1.0);
        assert!(validate(&slices).is_ok());
    }

    #[test]
    fn typed_pixels_should_set_the_rectangle_and_the_corners() {
        let mut slices = added();
        let size = Size {
            width: 1920.0,
            height: 1080.0,
        };
        let input = |field, text: &str| MappingOp::SetInputValue {
            id: "s1".into(),
            field,
            text: text.into(),
            size,
        };
        apply(&mut slices, input(RectField::Width, "960"), String::new).unwrap();
        apply(&mut slices, input(RectField::X, " 1500 "), String::new).unwrap();
        let r = slices[0].input;
        // The rectangle stops at the right edge rather than shrinking.
        assert_eq!((r.x, r.width), (0.5, 0.5));
        assert!(apply(&mut slices, input(RectField::Y, "abc"), String::new).is_err());
        let corner = MappingOp::SetCornerValue {
            id: "s1".into(),
            corner: 2,
            axis: Axis::Y,
            text: "540".into(),
            size,
        };
        apply(&mut slices, corner, String::new).unwrap();
        assert_eq!(slices[0].corners[2], Point { x: 1.0, y: 0.5 });
        let missing = MappingOp::SetCornerValue {
            id: "s1".into(),
            corner: 4,
            axis: Axis::X,
            text: "0".into(),
            size,
        };
        assert!(apply(&mut slices, missing, String::new).is_err());
        assert!(validate(&slices).is_ok());
    }

    #[test]
    fn screens_should_keep_the_main_one() {
        let mut list = Vec::new();
        edit_screens(&mut list, ScreenOp::Add, || "s2".into()).unwrap();
        assert_eq!(
            list.iter()
                .map(|s| (s.id.as_str(), s.name.as_str()))
                .collect::<Vec<_>>(),
            [(MAIN_SCREEN, "Screen 1"), ("s2", "Screen 2")]
        );
        let remove_main = ScreenOp::Remove {
            id: MAIN_SCREEN.into(),
        };
        assert!(edit_screens(&mut list, remove_main, String::new).is_err());
        let rename = ScreenOp::Rename {
            id: "s2".into(),
            name: " Left wall ".into(),
        };
        edit_screens(&mut list, rename, String::new).unwrap();
        assert_eq!(list[1].name, "Left wall");
        apply(
            &mut screen_mut(&mut list, "s2").unwrap().slices,
            MappingOp::Add,
            || "a".into(),
        )
        .unwrap();
        assert!(validate_screens(&list).is_ok());
        edit_screens(&mut list, ScreenOp::Remove { id: "s2".into() }, String::new).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(screens(&[])[0].id, MAIN_SCREEN);
        assert!(screen_mut(&mut list, "gone").is_err());
    }

    #[test]
    fn soft_edges_should_take_percentages_and_a_curve() {
        let mut slices = added();
        let edge = |field, text: &str| MappingOp::SetSoftEdge {
            id: "s1".into(),
            field,
            text: text.into(),
        };
        apply(&mut slices, edge(EdgeField::Right, "15 %"), String::new).unwrap();
        apply(&mut slices, edge(EdgeField::Left, "250"), String::new).unwrap();
        apply(&mut slices, edge(EdgeField::Gamma, "9"), String::new).unwrap();
        let e = slices[0].soft_edge;
        assert_eq!((e.left, e.right, e.gamma), (1.0, 0.15, 5.0));
        assert!(apply(&mut slices, edge(EdgeField::Top, "x"), String::new).is_err());
        assert!(validate(&slices).is_ok());
        let json = serde_json::to_value(&slices[0]).unwrap();
        assert_eq!(json["softEdge"]["right"], 0.15f32 as f64);
        let plain = serde_json::to_value(&added()[0]).unwrap();
        assert!(plain.get("softEdge").is_none());
    }

    #[test]
    fn masks_should_keep_three_points_on_the_screen() {
        let mut masks = Vec::new();
        edit_masks(&mut masks, MaskOp::Add, || "m".into()).unwrap();
        assert_eq!(
            (masks[0].name.as_str(), masks[0].points.len()),
            ("Mask 1", 4)
        );
        let add = MaskOp::AddPoint {
            id: "m".into(),
            after: 3,
        };
        edit_masks(&mut masks, add, String::new).unwrap();
        // Between the last point and the first: the middle of the left side.
        assert_eq!(masks[0].points[4], Point { x: 0.4, y: 0.5 });
        let mut far = masks[0].points.clone();
        far[0] = Point { x: -3.0, y: 2.0 };
        let set = MaskOp::SetPoints {
            id: "m".into(),
            points: far,
        };
        edit_masks(&mut masks, set, String::new).unwrap();
        assert_eq!(masks[0].points[0], Point { x: 0.0, y: 1.0 });
        let fewer = MaskOp::SetPoints {
            id: "m".into(),
            points: vec![Point { x: 0.0, y: 0.0 }; 3],
        };
        assert!(edit_masks(&mut masks, fewer, String::new).is_err());
        for _ in 0..2 {
            let remove = MaskOp::RemovePoint {
                id: "m".into(),
                index: 0,
            };
            edit_masks(&mut masks, remove, String::new).unwrap();
        }
        let last = MaskOp::RemovePoint {
            id: "m".into(),
            index: 0,
        };
        assert!(edit_masks(&mut masks, last, String::new).is_err());
        assert!(validate_masks(&masks).is_ok());
    }

    #[test]
    fn validate_should_refuse_a_repeated_id_or_an_unknown_slice() {
        let mut slices = added();
        slices.push(slices[0].clone());
        assert!(validate(&slices).is_err());
        let mut one = added();
        let op = MappingOp::Remove { id: "nope".into() };
        assert!(apply(&mut one, op, String::new).is_err());
    }

    #[test]
    fn a_slice_of_an_old_file_should_read_a_default_test_card() {
        let slices = added();
        let mut json: serde_json::Value = serde_json::to_value(&slices[0]).unwrap();
        json.as_object_mut().unwrap().remove("testCard");
        let back: Slice = serde_json::from_value(json).unwrap();
        assert_eq!(back.test_card, TestCard::default());
        assert_eq!(
            (back.test_card.cabinet_width, back.test_card.cabinet_height),
            (128, 128)
        );
    }

    #[test]
    fn a_test_card_should_round_trip_and_keep_its_sides_in_range() {
        let mut slices = added();
        let card = TestCard {
            show: true,
            cabinet_width: 64,
            cabinet_height: 99_999,
            numbering: Numbering::Dotted,
            scheme: Scheme::Contrast,
            base: "#12ab9f".into(),
            mode: CardMode::Custom,
            show_logo: true,
            logo: "img1".into(),
            logo_size: 99,
            ..TestCard::default()
        };
        let op = MappingOp::SetTestCard {
            id: "s1".into(),
            card,
        };
        apply(&mut slices, op, String::new).unwrap();
        let set = &slices[0].test_card;
        assert_eq!((set.cabinet_width, set.cabinet_height), (64, 2048));
        assert_eq!(
            (set.show_logo, set.logo.as_str(), set.logo_size),
            (true, "img1", 60)
        );
        assert_eq!(set.base_rgb(), Some([0x12, 0xab, 0x9f]));
        let back: Slice =
            serde_json::from_str(&serde_json::to_string(&slices[0]).unwrap()).unwrap();
        assert_eq!(back, slices[0]);
        let bad = TestCard {
            base: "red".into(),
            ..TestCard::default()
        };
        let op = MappingOp::SetTestCard {
            id: "s1".into(),
            card: bad,
        };
        assert!(apply(&mut slices, op, String::new).is_err());
    }
}
