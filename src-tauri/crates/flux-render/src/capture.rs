//! Drawing the composition with no window and reading it back, for the recorder. [`Screen::capture`]
//! draws a frame exactly as a window shows the composition (the same passes, through the same
//! shader, into an 8-bit sRGB texture of the composition's size) and starts copying it to the CPU;
//! the copy is finished by a later call, so a render loop never waits for the GPU to hand a frame
//! over.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::{Item, Post, Screen, compose};

/// What a frame is read back in: what the window's sRGB surface holds.
pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Frames in flight at once; a capture that finds them all busy draws nothing.
const SLOTS: usize = 3;

const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// A frame read back from the GPU: `rgba` is `width * height` pixels of 8-bit sRGB, rows top to
/// bottom, and `tag` is what the caller gave [`Screen::capture`] when it asked for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub tag: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The bytes of a row of `width` pixels once padded to what a texture copy needs.
pub fn padded_row(width: u32) -> u32 {
    (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
}

/// `padded` (rows of [`padded_row`] bytes) as `width * height` pixels with no padding.
pub fn unpad(padded: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (row, stride) = (width as usize * 4, padded_row(width) as usize);
    let mut out = Vec::with_capacity(row * height as usize);
    for line in padded.chunks(stride).take(height as usize) {
        out.extend_from_slice(&line[..row]);
    }
    out
}

struct Slot {
    buffer: wgpu::Buffer,
    /// Whether the buffer has mapped yet (it is set by the GPU's callback).
    state: Arc<AtomicU8>,
    /// The tag of the frame being copied into it; `None` when it is free.
    tag: Option<u64>,
}

/// The target a frame is drawn on and the buffers it is copied into.
pub(crate) struct Capture {
    size: (u32, u32),
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    slots: Vec<Slot>,
}

impl Capture {
    fn new(device: &wgpu::Device, size: (u32, u32)) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("capture"),
                    size: u64::from(padded_row(size.0)) * u64::from(size.1),
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(PENDING)),
                tag: None,
            })
            .collect();
        Self {
            size,
            texture,
            view,
            slots,
        }
    }

    /// The frames whose copies have finished, oldest first; their slots are free again.
    fn harvest(&mut self) -> Vec<Captured> {
        let (width, height) = self.size;
        let mut done = Vec::new();
        for slot in &mut self.slots {
            let Some(tag) = slot.tag else { continue };
            match slot.state.load(Ordering::Acquire) {
                MAPPED => {
                    if let Ok(view) = slot.buffer.slice(..).get_mapped_range() {
                        done.push(Captured {
                            tag,
                            width,
                            height,
                            rgba: unpad(&view, width, height),
                        });
                    }
                    slot.buffer.unmap();
                }
                FAILED => {}
                _ => continue,
            }
            slot.tag = None;
        }
        done.sort_by_key(|frame| frame.tag);
        done
    }
}

impl Screen {
    /// Draws `items` as the composition of `composition` pixels, through `post`, as a window shows
    /// it, and starts reading the picture back under `tag`. The answer is the frames that were
    /// started by earlier calls and have arrived by now, oldest first; none when nothing has. When
    /// every buffer is still being read, this frame is not drawn at all (the caller sees its tag
    /// never come back). Only for a screen made with [`Screen::offscreen`].
    pub fn capture(
        &mut self,
        items: &[Item<'_>],
        composition: (u32, u32),
        post: &Post,
        tag: u64,
    ) -> Vec<Captured> {
        if self.surface.is_some() {
            return Vec::new();
        }
        let size = compose::work_size(composition, self.device.limits().max_texture_dimension_2d);
        let mut done = Vec::new();
        let mut capture = match self.capture.take() {
            Some(capture) if capture.size == size => capture,
            Some(mut old) => {
                done = finish(&self.device, &mut old);
                Capture::new(&self.device, size)
            }
            None => Capture::new(&self.device, size),
        };
        // Mapping finishes inside a poll, which is not allowed to wait here.
        let _ = self.device.poll(wgpu::PollType::Poll);
        done.extend(capture.harvest());
        if capture.slots.iter().any(|slot| slot.tag.is_none()) {
            self.draw_into(&mut capture, items, composition, post, tag);
        }
        self.capture = Some(capture);
        done
    }

    /// Draws a frame into `capture` and starts copying it into a free slot.
    fn draw_into(
        &mut self,
        capture: &mut Capture,
        items: &[Item<'_>],
        composition: (u32, u32),
        post: &Post,
        tag: u64,
    ) {
        let size = capture.size;
        (self.config.width, self.config.height) = size;
        let staged = self.stage(items, composition, &[], &[], post);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.draw_composition(
            &mut encoder,
            &staged.steps,
            staged.program.clone(),
            post,
            composition,
        );
        self.present_pass(&mut encoder, &capture.view, items, staged.result);
        let Some(slot) = capture.slots.iter_mut().find(|slot| slot.tag.is_none()) else {
            return;
        };
        encoder.copy_texture_to_buffer(
            capture.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &slot.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row(size.0)),
                    rows_per_image: Some(size.1),
                },
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        slot.state.store(PENDING, Ordering::Release);
        slot.tag = Some(tag);
        let state = Arc::clone(&slot.state);
        slot.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                state.store(
                    if result.is_ok() { MAPPED } else { FAILED },
                    Ordering::Release,
                );
            });
    }

    /// Waits for the frames still being read and answers them, oldest first. It blocks until the
    /// GPU is done, so call it once, when the capture ends.
    pub fn capture_finish(&mut self) -> Vec<Captured> {
        match self.capture.as_mut() {
            Some(capture) => finish(&self.device, capture),
            None => Vec::new(),
        }
    }
}

fn finish(device: &wgpu::Device, capture: &mut Capture) -> Vec<Captured> {
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    capture.harvest()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_should_pad_to_256_bytes() {
        assert_eq!(padded_row(64), 256);
        assert_eq!(padded_row(65), 512);
        assert_eq!(padded_row(1920), 7680);
        assert_eq!(padded_row(1), 256);
    }

    #[test]
    fn unpad_should_drop_the_padding_of_each_row() {
        // 3 pixels wide: 12 bytes of picture in rows of 256.
        let mut padded = vec![0u8; 256 * 2];
        padded[..12].copy_from_slice(&[1; 12]);
        padded[256..268].copy_from_slice(&[2; 12]);
        let out = unpad(&padded, 3, 2);
        assert_eq!(out.len(), 24);
        assert_eq!(&out[..12], &[1; 12]);
        assert_eq!(&out[12..], &[2; 12]);
    }

    /// A solid red picture drawn offscreen comes back red, with the size asked for; skipped when
    /// the machine has no graphics adapter.
    #[test]
    fn a_captured_frame_should_hold_what_was_drawn() {
        let Ok(mut screen) = Screen::offscreen() else {
            eprintln!("no graphics adapter: capture test skipped");
            return;
        };
        let rgba = [255u8, 0, 0, 255].repeat(16);
        let picture = crate::Picture {
            id: 1,
            width: 4,
            height: 4,
            rgba: &rgba,
            place: [0.0, 0.0, 1.0, 1.0],
            opacity: 1.0,
            blend: crate::Blend::Alpha,
            effects: Default::default(),
            nearest: false,
        };
        let items = [Item::Picture(picture)];
        let mut frames = screen.capture(&items, (60, 34), &Post::default(), 7);
        frames.extend(screen.capture_finish());
        let frame = frames.first().expect("the frame arrives");
        assert_eq!((frame.tag, frame.width, frame.height), (7, 60, 34));
        assert_eq!(frame.rgba.len(), 60 * 34 * 4);
        let middle = &frame.rgba[(17 * 60 + 30) * 4..][..4];
        assert!(
            middle[0] > 250 && middle[1] < 5 && middle[2] < 5,
            "{middle:?}"
        );
    }
}
