use super::*;

/// A glyph of a line: its face, its id there, and where it sits against the start of the line and
/// its baseline (up is positive).
pub(super) struct Placed {
    pub(super) face: Arc<Face>,
    pub(super) id: GlyphId,
    pub(super) x: f32,
    pub(super) up: f32,
}

/// One line of glyphs, and how long the line is.
pub(super) struct Line {
    pub(super) glyphs: Vec<Placed>,
    pub(super) width: f32,
}

/// Whether `ch` joins the glyph before it instead of having a face of its own: a zero width joiner
/// or mark, or a variation selector, which no font has in its cmap.
pub(super) fn joins(ch: char) -> bool {
    matches!(ch, '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// `line` cut into runs of the same face, as `face_of` picks it for each character.
pub(super) fn split_runs<F: PartialEq>(
    line: &str,
    mut face_of: impl FnMut(char) -> F,
) -> Vec<(F, &str)> {
    let mut runs: Vec<(F, usize, usize)> = Vec::new();
    for (at, ch) in line.char_indices() {
        let end = at + ch.len_utf8();
        match runs.last_mut() {
            Some(last) if joins(ch) => last.2 = end,
            _ => {
                let face = face_of(ch);
                match runs.last_mut() {
                    Some(last) if last.0 == face => last.2 = end,
                    _ => runs.push((face, at, end)),
                }
            }
        }
    }
    runs.into_iter()
        .map(|(face, from, to)| (face, &line[from..to]))
        .collect()
}

/// The face that draws `ch`: the chosen one, else one of the system that has it, else the bundled
/// Inter; the chosen one again (its blank glyph) when nobody has it.
pub(super) fn face_for(chosen: &Arc<Face>, want: (u16, Style), ch: char) -> Arc<Face> {
    if chosen.has(ch) || ch.is_whitespace() {
        return chosen.clone();
    }
    if let Some(found) = fonts().covering(ch, want) {
        return found;
    }
    let inter = bundled(want.0);
    if inter.has(ch) { inter } else { chosen.clone() }
}

pub(super) fn layout_line(
    chosen: &Arc<Face>,
    want: (u16, Style),
    em: f32,
    line: &str,
    tracking: f32,
) -> Line {
    use rustybuzz::{Feature, UnicodeBuffer, ttf_parser::Tag};
    let mut glyphs = Vec::with_capacity(line.len());
    let mut pen = 0.0;
    // Space between glyphs would show a ligature as one stuck glyph: ligatures are off then.
    let off = |tag: &[u8; 4]| Feature::new(Tag::from_bytes(tag), 0, ..);
    let features: &[Feature] = &[off(b"liga"), off(b"clig")];
    let features = if tracking == 0.0 { &[] } else { features };
    for (face, run) in split_runs(line, |ch| face_for(chosen, want, ch)) {
        let k = em / face.upem;
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(run);
        buffer.guess_segment_properties();
        let shaped = rustybuzz::shape(&face.shaper, features, buffer);
        for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            glyphs.push(Placed {
                face: face.clone(),
                id: GlyphId(info.glyph_id as u16),
                x: pen + pos.x_offset as f32 * k,
                up: pos.y_offset as f32 * k,
            });
            pen += pos.x_advance as f32 * k + tracking;
        }
    }
    Line {
        glyphs,
        width: if line.is_empty() { 0.0 } else { pen - tracking },
    }
}
