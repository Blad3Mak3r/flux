//! Small pictures of a video, for its clips on the timeline: one every second or so, side by side in
//! one JPEG strip, so the window loads a single image per video.

use std::path::Path;

use crate::decode::{VideoError, VideoFile};

/// Height of a thumbnail in pixels; its width follows the shape of the video.
pub const THUMB_HEIGHT: u32 = 54;
/// A JPEG is at most 65 535 pixels wide, so a long video gets fewer, further apart.
const MAX_THUMBS: u64 = 600;

/// A strip of thumbnails: `count` pictures of `width` x `height`, left to right, the `i`th of
/// them showing frame `i * every` of the video.
#[derive(Debug, Clone)]
pub struct Thumbs {
    pub width: u32,
    pub height: u32,
    pub count: u32,
    /// Frames of the video between two thumbnails.
    pub every: u64,
    /// Seconds of the video between two thumbnails.
    pub seconds: f64,
    pub jpeg: Vec<u8>,
}

/// Decodes the thumbnails of the video at `path`. It seeks to each one, so it reads a fraction of a
/// long video; it still takes a moment, so call it off the UI thread.
pub fn thumbnails(path: &Path) -> Result<Thumbs, VideoError> {
    let mut file = VideoFile::open(path)?;
    let info = file.info().clone();
    let height = THUMB_HEIGHT;
    let width = (u64::from(info.width) * u64::from(height))
        .div_ceil(u64::from(info.height.max(1)))
        .clamp(1, u64::from(height) * 4) as u32;
    let per_second = u64::from(info.rate.num).div_ceil(u64::from(info.rate.den.max(1)));
    let every = per_second.max(info.frames.div_ceil(MAX_THUMBS)).max(1);
    let count = info.frames.div_ceil(every).max(1);
    file.set_output_size(width, height);

    let row = width as usize * 4;
    let strip_row = row * count as usize;
    let mut strip = vec![0u8; strip_row * height as usize];
    for i in 0..count {
        // A frame that cannot be decoded leaves its place black.
        let Ok(frame) = file.frame((i * every).min(info.frames.saturating_sub(1))) else {
            continue;
        };
        for y in 0..height as usize {
            let from = &frame.rgba[y * row..(y + 1) * row];
            let at = y * strip_row + i as usize * row;
            let to = &mut strip[at..at + row];
            to.copy_from_slice(from);
            // A transparent picture is shown over black, as the output shows it on its own.
            if frame.alpha {
                for pixel in to.as_chunks_mut::<4>().0 {
                    let a = u16::from(pixel[3]);
                    for c in &mut pixel[..3] {
                        *c = (u16::from(*c) * a / 255) as u8;
                    }
                }
            }
        }
    }
    let mut jpeg = Vec::new();
    jpeg_encoder::Encoder::new(&mut jpeg, 70)
        .encode(
            &strip,
            u16::try_from(width as u64 * count).unwrap_or(u16::MAX),
            height as u16,
            jpeg_encoder::ColorType::Rgba,
        )
        .map_err(|err| VideoError::Encode(err.to_string()))?;
    Ok(Thumbs {
        width,
        height,
        count: count as u32,
        every,
        seconds: every as f64 * f64::from(info.rate.den) / f64::from(info.rate.num.max(1)),
        jpeg,
    })
}
