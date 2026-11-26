//! Generators: clips of the VJ grid whose picture Flux makes itself instead of reading a video, such
//! as a solid colour or a strobe. A generator fills the composition, and its parameters are stored
//! with the clip; a parameter missing from a file takes its default, so a new clip is named by its
//! kind alone.

use serde::{Deserialize, Serialize};

use crate::{ProjectError, Result};

/// Fastest a strobe flashes or a noise changes, a second.
pub const MAX_RATE: f32 = 30.0;
/// The size of a text block, as fractions of the composition's height.
const TEXT_SIZE: (f32, f32) = (0.01, 2.0);
/// How far the centre of a text block can be, as fractions of the composition.
const TEXT_POSITION: (f32, f32) = (-1.0, 2.0);
/// The tracking and leading of a text block, in ems.
const TEXT_SPACE: (f32, f32) = (-0.5, 2.0);

/// Most characters a text block holds.
pub const MAX_TEXT: usize = 4000;
/// Longest a font family or style name is.
const MAX_NAME: usize = 128;
/// What a text block's advance can be, in beats; 0 is by hand.
pub const ADVANCES: [u32; 6] = [0, 1, 2, 4, 8, 16];

/// How the lines of a text block sit against each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// Whether a text block shows all its lines or one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextLines {
    #[default]
    All,
    One,
}

/// What a generator draws, with its parameters. Colours are `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Generator {
    /// One colour over the whole composition.
    Solid {
        #[serde(default = "white")]
        colour: String,
    },
    /// Flashes of `colour`, `rate` a second, each lasting `width` of the time between two (0 to 1);
    /// between flashes what is below shows.
    Strobe {
        #[serde(default = "white")]
        colour: String,
        #[serde(default = "strobe_rate")]
        rate: f32,
        #[serde(default = "half")]
        width: f32,
    },
    /// From `from` to `to` across the composition, along `angle` degrees: 0 is left to right, 90
    /// top to bottom.
    Gradient {
        #[serde(default = "black")]
        from: String,
        #[serde(default = "white")]
        to: String,
        #[serde(default)]
        angle: f32,
    },
    /// The colour bars of a video test card.
    Bars,
    /// Random snow of `colour`, new `rate` times a second.
    Noise {
        #[serde(default = "white")]
        colour: String,
        #[serde(default = "noise_rate")]
        rate: f32,
    },
    /// A block of text over a transparent background, as Resolume's Text Block. `size` is the height
    /// of the font as a fraction of the composition's, `position_x` and `position_y` the centre of
    /// the block as fractions of its width and height, `rotation` degrees clockwise, `tracking` and
    /// `leading` extra space between glyphs and between lines in ems. With `lines` `one`, `line` is
    /// the line shown and `advance` steps it to the next every that many beats (0 is by hand), so
    /// the line shown goes round the lines that are not empty.
    #[serde(rename_all = "camelCase")]
    Text {
        #[serde(default = "text")]
        text: String,
        #[serde(default = "font")]
        font: String,
        #[serde(default = "style")]
        style: String,
        #[serde(default = "text_size")]
        size: f32,
        #[serde(default = "white")]
        colour: String,
        #[serde(default = "one")]
        opacity: f32,
        #[serde(default)]
        align: TextAlign,
        #[serde(default = "half")]
        position_x: f32,
        #[serde(default = "half")]
        position_y: f32,
        #[serde(default)]
        rotation: f32,
        #[serde(default)]
        tracking: f32,
        #[serde(default)]
        leading: f32,
        #[serde(default)]
        lines: TextLines,
        #[serde(default)]
        line: u32,
        #[serde(default)]
        advance: u32,
    },
}

fn white() -> String {
    "#ffffff".into()
}

fn black() -> String {
    "#000000".into()
}

fn strobe_rate() -> f32 {
    8.0
}

fn noise_rate() -> f32 {
    MAX_RATE
}

fn half() -> f32 {
    0.5
}

fn one() -> f32 {
    1.0
}

fn text() -> String {
    "Text".into()
}

fn font() -> String {
    "Inter".into()
}

fn style() -> String {
    "Bold".into()
}

fn text_size() -> f32 {
    0.12
}

impl Generator {
    /// Its colours, to check.
    fn colours(&self) -> Vec<&str> {
        match self {
            Self::Solid { colour }
            | Self::Strobe { colour, .. }
            | Self::Noise { colour, .. }
            | Self::Text { colour, .. } => vec![colour],
            Self::Gradient { from, to, .. } => vec![from, to],
            Self::Bars => Vec::new(),
        }
    }

    /// Itself with each number brought into its range, as a fader stands; a number that is not one
    /// stays, for [`Generator::validate`] to refuse.
    pub(crate) fn tidy(mut self) -> Self {
        let into = |v: &mut f32, min: f32, max: f32| *v = v.clamp(min, max);
        // The smallest rate and width that still flash.
        const LEAST: f32 = 0.01;
        match &mut self {
            Self::Strobe { rate, width, .. } => {
                into(rate, LEAST, MAX_RATE);
                into(width, LEAST, 1.0);
            }
            Self::Noise { rate, .. } => into(rate, LEAST, MAX_RATE),
            Self::Gradient { angle, .. } => into(angle, -360.0, 360.0),
            Self::Text {
                text,
                size,
                opacity,
                position_x,
                position_y,
                rotation,
                tracking,
                leading,
                advance,
                ..
            } => {
                // Lines end in `\n`, a tab is a space, and no other control character is text.
                *text = text
                    .replace("\r\n", "\n")
                    .chars()
                    .filter_map(|c| match c {
                        '\r' => Some('\n'),
                        '\t' => Some(' '),
                        c if c.is_control() && c != '\n' => None,
                        c => Some(c),
                    })
                    .collect();
                into(size, TEXT_SIZE.0, TEXT_SIZE.1);
                into(opacity, 0.0, 1.0);
                into(position_x, TEXT_POSITION.0, TEXT_POSITION.1);
                into(position_y, TEXT_POSITION.0, TEXT_POSITION.1);
                into(rotation, -360.0, 360.0);
                into(tracking, TEXT_SPACE.0, TEXT_SPACE.1);
                into(leading, TEXT_SPACE.0, TEXT_SPACE.1);
                // The nearest step that exists.
                if let Some(&nearest) = ADVANCES.iter().min_by_key(|a| a.abs_diff(*advance)) {
                    *advance = nearest;
                }
            }
            Self::Solid { .. } | Self::Bars => {}
        }
        self
    }

    /// Checks its parameters: known colours, a rate above 0 up to [`MAX_RATE`], a flash width
    /// above 0 up to 1, an angle of -360 to 360 and a text block's text and numbers.
    pub(crate) fn validate(&self) -> Result<()> {
        if let Some(bad) = self.colours().into_iter().find(|c| rgb(c).is_none()) {
            return Err(ProjectError::Invalid(format!(
                "a generator colour must look like #rrggbb: {bad}"
            )));
        }
        if let Self::Text {
            text, font, style, ..
        } = self
        {
            if text.chars().count() > MAX_TEXT {
                return Err(ProjectError::Invalid(format!(
                    "a text block holds at most {MAX_TEXT} characters"
                )));
            }
            let bad =
                |name: &str| name.chars().count() > MAX_NAME || name.chars().any(char::is_control);
            if text.chars().any(|c| c.is_control() && c != '\n') || bad(font) || bad(style) {
                return Err(ProjectError::Invalid(
                    "a text block's text and font cannot hold control characters".into(),
                ));
            }
        }
        let fine = match *self {
            Self::Strobe { rate, width, .. } => {
                rate > 0.0 && rate <= MAX_RATE && width > 0.0 && width <= 1.0
            }
            Self::Noise { rate, .. } => rate > 0.0 && rate <= MAX_RATE,
            Self::Gradient { angle, .. } => (-360.0..=360.0).contains(&angle),
            Self::Text {
                size,
                opacity,
                position_x,
                position_y,
                rotation,
                tracking,
                leading,
                advance,
                ..
            } => {
                TEXT_SIZE.0 <= size
                    && size <= TEXT_SIZE.1
                    && (0.0..=1.0).contains(&opacity)
                    && [position_x, position_y]
                        .iter()
                        .all(|p| (TEXT_POSITION.0..=TEXT_POSITION.1).contains(p))
                    && (-360.0..=360.0).contains(&rotation)
                    && [tracking, leading]
                        .iter()
                        .all(|s| (TEXT_SPACE.0..=TEXT_SPACE.1).contains(s))
                    && ADVANCES.contains(&advance)
            }
            Self::Solid { .. } | Self::Bars => true,
        };
        if fine {
            Ok(())
        } else {
            Err(ProjectError::Invalid(format!(
                "a generator needs a rate of 0 to {MAX_RATE}, a width of 0 to 1, an angle of -360 to 360, and a text block a size of {} to {}, an opacity of 0 to 1, a position of {} to {}, spacing of {} to {} and an advance of 0, 1, 2, 4, 8 or 16 beats",
                TEXT_SIZE.0,
                TEXT_SIZE.1,
                TEXT_POSITION.0,
                TEXT_POSITION.1,
                TEXT_SPACE.0,
                TEXT_SPACE.1
            )))
        }
    }
}

impl Generator {
    /// What a text block draws `beats` into the grid's clock: all its text, or the one line its
    /// `line` and `advance` pick; `None` for another generator and when there is nothing to show.
    pub fn shown_text(&self, beats: f64) -> Option<String> {
        let Self::Text {
            text,
            lines,
            line,
            advance,
            ..
        } = self
        else {
            return None;
        };
        if text.trim().is_empty() {
            return None;
        }
        match lines {
            TextLines::All => Some(text.clone()),
            TextLines::One => {
                let steps = if *advance == 0 || beats.is_nan() || beats <= 0.0 {
                    0
                } else {
                    (beats / f64::from(*advance)).floor() as u64
                };
                let at = shown_line(text, *line, steps)?;
                text.split('\n').nth(at).map(str::to_owned)
            }
        }
    }
}

/// The indices of the lines of `text` that are not empty.
fn filled(text: &str) -> Vec<usize> {
    text.split('\n')
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(i, _)| i)
        .collect()
}

/// Where `line` stands among the filled ones: itself, or the next one after it, wrapping.
fn place(filled: &[usize], line: u32) -> usize {
    filled.iter().position(|&i| i >= line as usize).unwrap_or(0)
}

/// The line of `text` shown `steps` lines after `line`, skipping the empty ones and wrapping round;
/// `None` when every line is empty.
pub fn shown_line(text: &str, line: u32, steps: u64) -> Option<usize> {
    let filled = filled(text);
    let len = filled.len() as u64;
    (len > 0).then(|| filled[((place(&filled, line) as u64 + steps % len) % len) as usize])
}

/// `line` moved `by` lines (backwards when negative), skipping the empty ones and wrapping round;
/// `line` itself when every line is empty.
pub fn step_line(text: &str, line: u32, by: i32) -> u32 {
    let filled = filled(text);
    let Some(len) = i64::try_from(filled.len()).ok().filter(|&n| n > 0) else {
        return line;
    };
    let to = (place(&filled, line) as i64 + i64::from(by)).rem_euclid(len);
    filled[to as usize] as u32
}

/// The red, green and blue of `#rrggbb`.
pub fn rgb(colour: &str) -> Option<[u8; 3]> {
    let digits = colour.strip_prefix('#')?;
    if digits.len() != 6 || !digits.is_ascii() {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_alone_should_read_with_the_default_parameters() {
        let strobe: Generator = serde_json::from_str(r#"{"kind":"strobe"}"#).unwrap();
        assert_eq!(
            strobe,
            Generator::Strobe {
                colour: "#ffffff".into(),
                rate: 8.0,
                width: 0.5
            }
        );
        assert!(strobe.validate().is_ok());
        let bars: Generator = serde_json::from_str(r#"{"kind":"bars"}"#).unwrap();
        assert_eq!(bars, Generator::Bars);
    }

    #[test]
    fn validate_should_refuse_bad_colours_and_rates() {
        let colour = Generator::Solid {
            colour: "red".into(),
        };
        assert!(colour.validate().is_err());
        for rate in [0.0, MAX_RATE + 1.0, f32::NAN] {
            let noise = Generator::Noise {
                colour: "#ffffff".into(),
                rate,
            };
            assert!(noise.validate().is_err(), "{rate}");
        }
    }

    #[test]
    fn tidy_should_bring_numbers_into_range() {
        let fast = Generator::Strobe {
            colour: "#ffffff".into(),
            rate: 100.0,
            width: -1.0,
        };
        assert!(fast.clone().tidy().validate().is_ok());
        let nan = Generator::Gradient {
            from: "#000000".into(),
            to: "#ffffff".into(),
            angle: f32::NAN,
        };
        assert!(nan.tidy().validate().is_err());
    }

    fn text(text: &str) -> Generator {
        serde_json::from_value(serde_json::json!({ "kind": "text", "text": text })).unwrap()
    }

    #[test]
    fn a_text_block_should_read_with_its_defaults_and_round_trip() {
        let block: Generator = serde_json::from_str(r#"{"kind":"text"}"#).unwrap();
        assert_eq!(
            block,
            Generator::Text {
                text: "Text".into(),
                font: "Inter".into(),
                style: "Bold".into(),
                size: 0.12,
                colour: "#ffffff".into(),
                opacity: 1.0,
                align: TextAlign::Center,
                position_x: 0.5,
                position_y: 0.5,
                rotation: 0.0,
                tracking: 0.0,
                leading: 0.0,
                lines: TextLines::All,
                line: 0,
                advance: 0,
            }
        );
        assert!(block.validate().is_ok());
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["positionX"], 0.5);
        assert_eq!(json["align"], "center");
        assert_eq!(serde_json::from_value::<Generator>(json).unwrap(), block);
    }

    #[test]
    fn validate_should_refuse_a_long_text_and_numbers_out_of_range() {
        assert!(text(&"a".repeat(MAX_TEXT)).validate().is_ok());
        assert!(text(&"a".repeat(MAX_TEXT + 1)).validate().is_err());
        assert!(text("a\u{7}b").validate().is_err());
        let bad = |field: &str, value: serde_json::Value| {
            let mut json = serde_json::json!({ "kind": "text" });
            json[field] = value;
            serde_json::from_value::<Generator>(json)
                .unwrap()
                .validate()
        };
        assert!(bad("size", 0.0.into()).is_err());
        assert!(bad("opacity", 1.5.into()).is_err());
        assert!(bad("advance", 3.into()).is_err());
        assert!(bad("positionX", 9.0.into()).is_err());
        assert!(bad("font", "x".repeat(200).into()).is_err());
        assert!(bad("size", 0.5.into()).is_ok());
    }

    #[test]
    fn tidy_should_clean_a_text_and_bring_its_numbers_in() {
        let Generator::Text {
            text,
            size,
            advance,
            ..
        } = serde_json::from_value::<Generator>(serde_json::json!({
            "kind": "text", "text": "a\r\nb\tc\u{7}", "size": 9.0, "advance": 5
        }))
        .unwrap()
        .tidy()
        else {
            unreachable!()
        };
        assert_eq!((text.as_str(), size, advance), ("a\nb c", 2.0, 4));
    }

    #[test]
    fn lines_should_step_by_hand_skipping_empty_ones_and_wrapping() {
        let text = "one\n\ntwo\n  \nthree";
        assert_eq!(step_line(text, 0, 1), 2);
        assert_eq!(step_line(text, 2, 1), 4);
        assert_eq!(step_line(text, 4, 1), 0, "wraps");
        assert_eq!(step_line(text, 0, -1), 4, "wraps back");
        assert_eq!(
            step_line(text, 1, 1),
            4,
            "from an empty line, the next one is two's successor"
        );
        assert_eq!(step_line("\n\n", 1, 1), 1, "nothing to step to");
    }

    #[test]
    fn a_single_line_should_follow_the_beats_by_its_advance() {
        let block = |advance: u32, line: u32| {
            serde_json::from_value::<Generator>(serde_json::json!({
                "kind": "text", "text": "a\n\nb\nc", "lines": "one", "line": line, "advance": advance
            }))
            .unwrap()
        };
        let shown = |g: &Generator, beats: f64| g.shown_text(beats).unwrap();
        let manual = block(0, 2);
        assert_eq!(
            (shown(&manual, 0.0), shown(&manual, 99.0)),
            ("b".into(), "b".into())
        );
        let every_two = block(2, 0);
        let seen: Vec<String> = [0.0, 1.9, 2.0, 4.0, 6.0, 8.0]
            .map(|b| shown(&every_two, b))
            .into();
        assert_eq!(
            seen,
            ["a", "a", "b", "c", "a", "b"],
            "steps, skipping the empty line, wrapping"
        );
        assert_eq!(shown(&block(1, 3), 1.0), "a", "from the last line it wraps");
        assert_eq!(shown(&block(1, 0), -4.0), "a", "no beats before the start");
        assert_eq!(text("   ").shown_text(0.0), None);
        assert_eq!(
            text("x\ny").shown_text(0.0).as_deref(),
            Some("x\ny"),
            "all lines"
        );
    }

    #[test]
    fn rgb_should_read_a_hex_triplet() {
        assert_eq!(rgb("#ff8000"), Some([255, 128, 0]));
        assert_eq!(rgb("#ff80"), None);
        assert_eq!(rgb("ff8000"), None);
        assert_eq!(rgb("#ééé"), None);
    }
}
