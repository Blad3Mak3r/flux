//! Converting a video to HAP, the codec media servers play: every frame is a picture of its own,
//! so a seek costs one frame, and decoding is light enough for several layers at full size. The
//! frames are read the way Flux reads the original, so frame `n` of the copy is frame `n` of the
//! source; its sound is kept beside them as 16-bit PCM.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::format::context::{Input, Output};
use ffmpeg_next::format::sample::{Sample, Type as Layout};
use ffmpeg_next::format::{self, Pixel};
use ffmpeg_next::media::Type;
use ffmpeg_next::{ChannelLayout, Dictionary, Packet, Rational, codec, decoder, encoder, frame};

use crate::audio::Pcm;
use crate::decode::{Frame, VideoError, VideoFile};

/// Whether this build of FFmpeg can write HAP (it needs the snappy library).
pub fn hap_available() -> bool {
    crate::init().is_ok() && encoder::find_by_name("hap").is_some()
}

/// Writes a HAP copy of the video at `src` to `dst` (a QuickTime file): HAP Alpha when the source
/// has transparency, HAP otherwise, at the source's size rounded up to a multiple of 4 (what HAP's
/// textures need). `progress` hears the frames done and the total; setting `cancel` stops it with
/// [`VideoError::Cancelled`]. Nothing is left at `dst` when it fails. It reads and writes the whole
/// file, so call it off the UI thread.
pub fn convert_to_hap(
    src: &Path,
    dst: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), VideoError> {
    let result = convert(src, dst, progress, cancel);
    if result.is_err() {
        let _ = fs::remove_file(dst);
    }
    result
}

fn convert(
    src: &Path,
    dst: &Path,
    progress: &mut dyn FnMut(u64, u64),
    cancel: &AtomicBool,
) -> Result<(), VideoError> {
    let mut video = VideoFile::open(src)?;
    let info = video.info().clone();
    let (width, height) = (info.width.div_ceil(4) * 4, info.height.div_ceil(4) * 4);
    video.set_output_size(width, height);
    let hap = encoder::find_by_name("hap").ok_or(VideoError::NoHap)?;

    let mut output = format::output_as(dst, "mov")?;
    let global = output
        .format()
        .flags()
        .contains(format::Flags::GLOBAL_HEADER);
    // One tick per frame: frame `n` is written at `n`.
    let time_base = Rational::new(info.rate.den as i32, info.rate.num as i32);
    let mut encoder = CodecContext::new_with_codec(hap).encoder().video()?;
    encoder.set_width(width);
    encoder.set_height(height);
    encoder.set_format(Pixel::RGBA);
    encoder.set_time_base(time_base);
    encoder.set_frame_rate(Some(Rational::new(
        info.rate.num as i32,
        info.rate.den as i32,
    )));
    if global {
        encoder.set_flags(codec::Flags::GLOBAL_HEADER);
    }
    let mut options = Dictionary::new();
    options.set("format", if info.alpha { "hap_alpha" } else { "hap" });
    let mut encoder = encoder.open_with(options)?;
    {
        let mut stream = output.add_stream(hap)?;
        stream.set_parameters(&encoder);
        stream.set_time_base(time_base);
    }
    let mut sound = if info.has_audio {
        Sound::open(src, &mut output, 1)?
    } else {
        None
    };
    output.write_header()?;
    let video_base = output.stream(0).map_or(time_base, |s| s.time_base());

    let mut picture = frame::Video::new(Pixel::RGBA, width, height);
    for index in 0..info.frames {
        if cancel.load(Ordering::Relaxed) {
            return Err(VideoError::Cancelled);
        }
        let decoded = match video.frame(index) {
            Ok(frame) => frame,
            // A count read from the container can run a frame or two past the last one decoded.
            Err(VideoError::PastEnd { .. }) => break,
            Err(err) => return Err(err),
        };
        fill(&mut picture, &decoded);
        picture.set_pts(Some(index as i64));
        encoder.send_frame(&picture)?;
        write_packets(&mut encoder, &mut output, time_base, video_base)?;
        if let Some(sound) = &mut sound {
            sound.until(
                (index + 1) as f64 * f64::from(info.rate.den) / f64::from(info.rate.num),
                &mut output,
            )?;
        }
        progress(index + 1, info.frames);
    }
    encoder.send_eof()?;
    write_packets(&mut encoder, &mut output, time_base, video_base)?;
    if let Some(sound) = &mut sound {
        sound.until(f64::INFINITY, &mut output)?;
    }
    output.write_trailer()?;
    Ok(())
}

/// Copies a decoded frame into the encoder's picture, row by row (the picture's rows may be wider).
fn fill(picture: &mut frame::Video, decoded: &Frame) {
    let row = decoded.width as usize * 4;
    let stride = picture.stride(0);
    let data = picture.data_mut(0);
    for (y, line) in decoded.rgba.chunks_exact(row).enumerate() {
        data[y * stride..y * stride + row].copy_from_slice(line);
    }
}

fn write_packets(
    encoder: &mut encoder::Video,
    output: &mut Output,
    from: Rational,
    to: Rational,
) -> Result<(), VideoError> {
    let mut packet = Packet::empty();
    while encoder.receive_packet(&mut packet).is_ok() {
        packet.set_stream(0);
        // Each frame lasts one tick, or the last one would end the file early and bend the rate.
        packet.set_duration(1);
        packet.rescale_ts(from, to);
        packet.write_interleaved(output)?;
    }
    Ok(())
}

/// The sound of the source, decoded as the mixer hears it and written as 16-bit PCM in step with
/// the frames, so the copy stays interleaved.
struct Sound {
    input: Input,
    index: usize,
    decoder: decoder::Audio,
    pcm: Pcm,
    stream: usize,
    time_base: Rational,
    /// Samples (per channel) written so far.
    written: i64,
    done: bool,
}

impl Sound {
    fn open(src: &Path, output: &mut Output, stream: usize) -> Result<Option<Self>, VideoError> {
        let input = format::input(src)?;
        let Some(source) = input.streams().best(Type::Audio) else {
            return Ok(None);
        };
        let index = source.index();
        let decoder = CodecContext::from_parameters(source.parameters())?
            .decoder()
            .audio()?;
        let (rate, channels) = (decoder.rate(), decoder.channels().clamp(1, 2));
        if rate == 0 {
            return Ok(None);
        }
        let pcm_codec = encoder::find(codec::Id::PCM_S16LE)
            .ok_or_else(|| VideoError::Encode("FFmpeg has no PCM encoder".into()))?;
        let mut pcm = CodecContext::new_with_codec(pcm_codec).encoder().audio()?;
        let time_base = Rational::new(1, rate as i32);
        pcm.set_rate(rate as i32);
        pcm.set_format(Sample::I16(Layout::Packed));
        pcm.set_channel_layout(ChannelLayout::default(i32::from(channels)));
        pcm.set_time_base(time_base);
        let pcm = pcm.open()?;
        let mut out = output.add_stream(pcm_codec)?;
        out.set_parameters(&pcm);
        out.set_time_base(time_base);
        Ok(Some(Self {
            input,
            index,
            decoder,
            pcm: Pcm::new(rate, channels),
            stream,
            time_base,
            written: 0,
            done: false,
        }))
    }

    /// Writes the sound up to `seconds` from its start (or to its end).
    fn until(&mut self, seconds: f64, output: &mut Output) -> Result<(), VideoError> {
        let rate = f64::from(self.pcm.rate);
        while !self.done && (self.written as f64) / rate < seconds {
            match self.input.packets().next() {
                Some((stream, packet)) if stream.index() == self.index => {
                    self.decoder.send_packet(&packet)?;
                    self.pcm.drain(&mut self.decoder)?;
                }
                Some(_) => continue,
                None => {
                    self.decoder.send_eof()?;
                    self.pcm.drain(&mut self.decoder)?;
                    self.done = true;
                }
            }
            self.flush(output)?;
        }
        Ok(())
    }

    fn flush(&mut self, output: &mut Output) -> Result<(), VideoError> {
        let samples = std::mem::take(&mut self.pcm.samples);
        if samples.is_empty() {
            return Ok(());
        }
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let frames = (samples.len() / usize::from(self.pcm.channels)) as i64;
        let mut packet = Packet::copy(&bytes);
        packet.set_stream(self.stream);
        packet.set_pts(Some(self.written));
        packet.set_dts(Some(self.written));
        packet.set_duration(frames);
        let to = output
            .stream(self.stream)
            .map_or(self.time_base, |s| s.time_base());
        packet.rescale_ts(self.time_base, to);
        packet.write_interleaved(output)?;
        self.written += frames;
        Ok(())
    }
}
