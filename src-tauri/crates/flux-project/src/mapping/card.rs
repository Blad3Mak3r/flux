//! The test card a slice can show, to line a projector up.

use serde::{Deserialize, Serialize};

use crate::{ProjectError, Result};

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

    pub(super) fn validate(&self) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mapping::*;

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
