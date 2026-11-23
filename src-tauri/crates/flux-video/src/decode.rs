use std::path::Path;

use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::format::{Pixel, context::Input};
use ffmpeg_next::media::Type;
use ffmpeg_next::software::scaling::{Context as Scaler, Flags};
use std::cmp::Ordering;

use ffmpeg_next::ffi::AVHWDeviceType;
use ffmpeg_next::util::error::EAGAIN;
use ffmpeg_next::{Rational, codec, decoder, frame};
use thiserror::Error;

use crate::mapping::Rate;

/// FFmpeg's own time base, in which a whole-file seek is given.
const AV_TIME_BASE: i64 = 1_000_000;

/// A target this many frames ahead or less is reached by decoding on; further away, by a seek.
/// It is about the gap between keyframes of a typical MP4, where seeking would not save work.
const DECODE_AHEAD: u64 = 48;

/// GPU frames the decoder keeps beyond what it needs itself: the frame before the one asked for
/// and one decoded past it are held while a frame is chosen, and a fixed pool (D3D11VA) would
/// otherwise run out.
const EXTRA_HW_FRAMES: i32 = 4;

/// The GPU decoders tried on this system, best first.
#[cfg(target_os = "windows")]
const DEVICES: &[AVHWDeviceType] = &[
    AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA,
    AVHWDeviceType::AV_HWDEVICE_TYPE_DXVA2,
];
#[cfg(target_os = "macos")]
const DEVICES: &[AVHWDeviceType] = &[AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const DEVICES: &[AVHWDeviceType] = &[
    AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
    AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
];

/// Where a file's frames are decoded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Decoding {
    /// On the CPU: the file was opened without acceleration, the codec has no GPU decoder here,
    /// or the GPU failed and decoding went on without it.
    #[default]
    Software,
    /// On the GPU, through FFmpeg's device of this name (`d3d11va`, `videotoolbox`, `vaapi`...).
    Hardware(&'static str),
}

#[derive(Debug, Error)]
pub enum VideoError {
    #[error("FFmpeg: {0}")]
    Ffmpeg(#[from] ffmpeg_next::Error),
    #[error("the file has no video stream")]
    NoVideo,
    #[error("the video stream has no frame rate")]
    NoRate,
    #[error("the decoding thread could not start: {0}")]
    Thread(#[from] std::io::Error),
    #[error("the thumbnails could not be encoded: {0}")]
    Encode(String),
    #[error("frame {index} is past the end of the video ({frames} frames)")]
    PastEnd { index: u64, frames: u64 },
}

/// What a video file holds, read when it is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub rate: Rate,
    /// Frames in the stream: the container's count, or its duration at `rate` when it has none.
    pub frames: u64,
    /// FFmpeg's short name of the codec, e.g. `h264`, `hevc`, `hap`, `prores`.
    pub codec: String,
    pub has_audio: bool,
    /// The picture carries transparency: an alpha channel (ProRes 4444, HAP Alpha, PNG, QuickTime
    /// Animation) or a WebM alpha track.
    pub alpha: bool,
}

/// One decoded picture, packed RGBA (4 bytes a pixel, rows without padding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub index: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// It has an alpha channel: where it is not opaque, what is below it shows.
    pub alpha: bool,
}

/// An open video file that hands out any of its frames by index, exactly: a frame far from the last
/// one decoded is reached by seeking to the keyframe before it and decoding up to it.
pub struct VideoFile {
    input: Input,
    stream: usize,
    decoder: decoder::Video,
    scaler: Option<Scaler>,
    /// The size frames are converted to; `None` keeps the video's own.
    output: Option<(u32, u32)>,
    time_base: Rational,
    start_pts: i64,
    info: VideoInfo,
    /// The last frame handed out, returned again when asked for twice.
    last: Option<Frame>,
    /// A frame decoded past the one asked for (a gap in the timestamps), kept for the next call.
    ahead: Option<(u64, frame::Video)>,
    /// Index of the last frame the decoder produced; `None` right after opening or seeking.
    decoded: Option<u64>,
    /// The frame the last seek aimed at.
    sought: Option<u64>,
    eof: bool,
    decoding: Decoding,
    /// The decoder holds a GPU device, even when FFmpeg went on on the CPU without it.
    gpu: bool,
}

impl VideoFile {
    /// Opens the file to decode on the CPU, as probing and thumbnails do.
    pub fn open(path: &Path) -> Result<Self, VideoError> {
        Self::open_with(path, false)
    }

    /// Opens the file to decode on the GPU when this computer has a decoder for its codec, and on
    /// the CPU otherwise; a GPU that fails later hands over to the CPU on its own.
    pub fn open_accelerated(path: &Path) -> Result<Self, VideoError> {
        Self::open_with(path, true)
    }

    fn open_with(path: &Path, accelerate: bool) -> Result<Self, VideoError> {
        crate::init()?;
        let input = ffmpeg_next::format::input(path)?;
        let stream = input
            .streams()
            .best(Type::Video)
            .ok_or(VideoError::NoVideo)?;
        let index = stream.index();
        let time_base = stream.time_base();
        let rate = [stream.avg_frame_rate(), stream.rate()]
            .into_iter()
            .find(|r| r.numerator() > 0 && r.denominator() > 0)
            .ok_or(VideoError::NoRate)?;
        let rate = Rate::new(
            rate.numerator().unsigned_abs(),
            rate.denominator().unsigned_abs(),
        );
        let start_pts = Some(stream.start_time())
            .filter(|&t| t != ffmpeg_next::ffi::AV_NOPTS_VALUE)
            .unwrap_or(0);
        let frames = u64::try_from(stream.frames())
            .ok()
            .filter(|&n| n > 0)
            .or_else(|| {
                Some(ticks_to_index(stream.duration().max(0), time_base, rate)).filter(|&n| n > 0)
            })
            // Matroska and WebM keep no count or duration per stream: the container's duration.
            .unwrap_or_else(|| {
                let micros = ffmpeg_next::Rational::new(1, ffmpeg_next::ffi::AV_TIME_BASE);
                ticks_to_index(input.duration().max(0), micros, rate)
            });
        let parameters = stream.parameters();
        // FFmpeg's own VP8 and VP9 decoders leave a WebM alpha track out; libvpx's reads it.
        let alpha_decoder = (stream.metadata().get("alpha_mode") == Some("1"))
            .then(|| match parameters.id() {
                codec::Id::VP8 => ffmpeg_next::decoder::find_by_name("libvpx"),
                codec::Id::VP9 => ffmpeg_next::decoder::find_by_name("libvpx-vp9"),
                _ => None,
            })
            .flatten();
        let (decoder, decoding) = match alpha_decoder {
            Some(alpha) => (
                CodecContext::from_parameters(parameters)?
                    .decoder()
                    .open_as(alpha)?
                    .video()?,
                Decoding::Software,
            ),
            None => open_decoder(parameters, accelerate)?,
        };
        let codec = decoder.id().name().to_owned();
        let has_audio = input.streams().best(Type::Audio).is_some();
        let info = VideoInfo {
            width: decoder.width(),
            height: decoder.height(),
            rate,
            frames,
            codec,
            has_audio,
            alpha: alpha_decoder.is_some() || has_alpha(decoder.format()),
        };
        Ok(Self {
            input,
            stream: index,
            decoder,
            scaler: None,
            output: None,
            time_base,
            start_pts,
            info,
            last: None,
            ahead: None,
            decoded: None,
            sought: None,
            eof: false,
            decoding,
            gpu: decoding != Decoding::Software,
        })
    }

    /// Where the frames are decoded now.
    pub fn decoding(&self) -> Decoding {
        self.decoding
    }

    /// Goes on decoding on the CPU after the GPU failed: the next frame is found by a seek.
    fn fall_back_to_software(&mut self) -> Result<(), VideoError> {
        let parameters = self
            .input
            .stream(self.stream)
            .ok_or(VideoError::NoVideo)?
            .parameters();
        (self.decoder, self.decoding) = open_decoder(parameters, false)?;
        self.gpu = false;
        self.scaler = None;
        self.ahead = None;
        self.decoded = None;
        self.eof = false;
        Ok(())
    }

    /// Converts the frames to `width` x `height` from now on, e.g. for thumbnails.
    pub fn set_output_size(&mut self, width: u32, height: u32) {
        self.output = Some((width.max(1), height.max(1)));
    }

    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    /// The frame at `index`, counted from 0 at the video's own rate. When the stream skips that
    /// index (a gap in variable frame rate material), the frame before it is the one shown.
    pub fn frame(&mut self, index: u64) -> Result<Frame, VideoError> {
        if index >= self.info.frames {
            return Err(VideoError::PastEnd {
                index,
                frames: self.info.frames,
            });
        }
        if let Some(last) = &self.last {
            if last.index == index {
                return Ok(last.clone());
            }
            // A frame kept from a gap is on screen until the next one starts.
            if let Some((next, _)) = &self.ahead
                && last.index < index
                && index < *next
            {
                return Ok(Frame {
                    index,
                    ..last.clone()
                });
            }
        }
        let frame = match self.decode(index) {
            Err(VideoError::Ffmpeg(_)) if self.gpu => {
                self.fall_back_to_software()?;
                self.decode(index)?
            }
            decoded => decoded?,
        };
        self.last = Some(frame.clone());
        Ok(frame)
    }

    fn decode(&mut self, index: u64) -> Result<Frame, VideoError> {
        let on_course = self
            .decoded
            .is_some_and(|at| at < index && index - at <= DECODE_AHEAD);
        if !on_course {
            self.seek(index)?;
        }
        let picture = self.decode_up_to(index)?;
        self.convert(index, &picture)
    }

    fn seek(&mut self, index: u64) -> Result<(), VideoError> {
        // FFmpeg tries the GPU again after every seek: once it has refused the stream, it is let go.
        if self.gpu && self.decoding == Decoding::Software {
            self.fall_back_to_software()?;
        }
        let pts = self.start_pts + index_to_ticks(index, self.time_base, self.info.rate);
        let ts = rescale(pts, self.time_base, Rational::new(1, AV_TIME_BASE as i32));
        // The keyframe at or before `ts` (FFmpeg takes the end of the range as inclusive).
        self.input.seek(ts, ..ts)?;
        self.decoder.flush();
        self.ahead = None;
        self.decoded = None;
        self.sought = Some(index);
        self.eof = false;
        Ok(())
    }

    /// Decodes until the frame at `index` or, if the stream has none there, the last one before it.
    fn decode_up_to(&mut self, index: u64) -> Result<frame::Video, VideoError> {
        let mut before = match self.ahead.take() {
            Some((at, picture)) if at == index => return Ok(picture),
            Some((at, picture)) if at < index => Some(picture),
            // Only a seek reaches an earlier frame.
            Some(_) => {
                self.seek(index)?;
                None
            }
            None => None,
        };
        loop {
            let mut picture = frame::Video::empty();
            match self.decoder.receive_frame(&mut picture) {
                Ok(()) => {
                    let at = self.index_of(&picture);
                    self.decoded = Some(at);
                    match at.cmp(&index) {
                        Ordering::Less => before = Some(picture),
                        Ordering::Equal => return Ok(picture),
                        Ordering::Greater => match before {
                            Some(shown) => {
                                self.ahead = Some((at, picture));
                                return Ok(shown);
                            }
                            None => return self.reseek_before(index, picture),
                        },
                    }
                }
                Err(ffmpeg_next::Error::Eof) => {
                    return before.ok_or(VideoError::PastEnd {
                        index,
                        frames: self.info.frames,
                    });
                }
                Err(ffmpeg_next::Error::Other { errno }) if errno == EAGAIN => self.feed()?,
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// The first frame after a seek is already past `index`: the container's keyframe index was
    /// off. Seeking further back and decoding again finds the frame; when the seek was already at
    /// the start, the stream begins after `index` and its first frame is the one shown.
    fn reseek_before(
        &mut self,
        index: u64,
        first: frame::Video,
    ) -> Result<frame::Video, VideoError> {
        let shown = self.sought.unwrap_or(index);
        if shown == 0 {
            return Ok(first);
        }
        self.seek(shown.saturating_sub(DECODE_AHEAD))?;
        self.decode_up_to(index)
    }

    /// Sends the decoder the next packet of the video stream, or the end of the file.
    fn feed(&mut self) -> Result<(), VideoError> {
        if self.eof {
            return Err(ffmpeg_next::Error::Eof.into());
        }
        for (stream, packet) in self.input.packets() {
            if stream.index() == self.stream {
                self.decoder.send_packet(&packet)?;
                return Ok(());
            }
        }
        self.eof = true;
        self.decoder.send_eof()?;
        Ok(())
    }

    fn index_of(&self, picture: &frame::Video) -> u64 {
        let pts = picture
            .timestamp()
            .or(picture.pts())
            .unwrap_or(self.start_pts);
        ticks_to_index(
            (pts - self.start_pts).max(0),
            self.time_base,
            self.info.rate,
        )
    }

    fn convert(&mut self, index: u64, picture: &frame::Video) -> Result<Frame, VideoError> {
        // A frame on the GPU is copied into memory first, in the GPU's own layout (NV12, P010).
        let copied;
        // SAFETY: `picture` is a valid frame; its `hw_frames_ctx` is only read.
        let picture = if unsafe { (*picture.as_ptr()).hw_frames_ctx.is_null() } {
            // FFmpeg goes on on the CPU by itself when the GPU cannot take this stream.
            self.decoding = Decoding::Software;
            picture
        } else {
            let mut memory = frame::Video::empty();
            // SAFETY: both frames are valid; FFmpeg allocates `memory`'s buffers.
            let result = unsafe {
                ffmpeg_next::ffi::av_hwframe_transfer_data(memory.as_mut_ptr(), picture.as_ptr(), 0)
            };
            if result < 0 {
                return Err(ffmpeg_next::Error::from(result).into());
            }
            copied = memory;
            &copied
        };
        let (source_width, source_height) = (picture.width(), picture.height());
        let (width, height) = self.output.unwrap_or((source_width, source_height));
        let scaler = match &mut self.scaler {
            Some(scaler)
                if scaler.input().width == source_width
                    && scaler.input().height == source_height
                    && scaler.input().format == picture.format()
                    && (scaler.output().width, scaler.output().height) == (width, height) =>
            {
                scaler
            }
            slot => slot.insert(Scaler::get(
                picture.format(),
                source_width,
                source_height,
                Pixel::RGBA,
                width,
                height,
                // Area averaging keeps small pictures from shimmering; same size is a copy.
                if self.output.is_some() {
                    Flags::AREA
                } else {
                    Flags::BILINEAR
                },
            )?),
        };
        let mut rgba = frame::Video::empty();
        scaler.run(picture, &mut rgba)?;
        let row = width as usize * 4;
        let stride = rgba.stride(0);
        let data = rgba.data(0);
        let mut packed = Vec::with_capacity(row * height as usize);
        for y in 0..height as usize {
            packed.extend_from_slice(&data[y * stride..y * stride + row]);
        }
        Ok(Frame {
            index,
            width,
            height,
            rgba: packed,
            alpha: has_alpha(picture.format()),
        })
    }
}

/// Whether pictures in `format` carry an alpha channel.
fn has_alpha(format: Pixel) -> bool {
    // SAFETY: the descriptors are static; an unknown format has none.
    let descriptor = unsafe { ffmpeg_next::ffi::av_pix_fmt_desc_get(format.into()).as_ref() };
    descriptor.is_some_and(|d| d.flags & ffmpeg_next::ffi::AV_PIX_FMT_FLAG_ALPHA as u64 != 0)
}

/// A decoder for a stream of `parameters`, on the GPU when `accelerate` and this computer has a
/// GPU decoder for its codec, and on the CPU otherwise.
fn open_decoder(
    parameters: codec::Parameters,
    accelerate: bool,
) -> Result<(decoder::Video, Decoding), VideoError> {
    let mut context = CodecContext::from_parameters(parameters.clone())?;
    let device = accelerate.then(|| gpu_device(context.id())).flatten();
    let Some((device, name)) = device else {
        return Ok((context.decoder().video()?, Decoding::Software));
    };
    // SAFETY: the context is not open yet; it takes over the device reference and frees it.
    unsafe {
        let raw = context.as_mut_ptr();
        (*raw).hw_device_ctx = device;
        (*raw).extra_hw_frames = EXTRA_HW_FRAMES;
    }
    match context.decoder().video() {
        Ok(decoder) => Ok((decoder, Decoding::Hardware(name))),
        Err(_) => open_decoder(parameters, false),
    }
}

/// A GPU device that the decoder of `codec` can decode on, with its name: the first of
/// [`DEVICES`] that the decoder supports and that opens on this computer.
fn gpu_device(codec: codec::Id) -> Option<(*mut ffmpeg_next::ffi::AVBufferRef, &'static str)> {
    let decoder = ffmpeg_next::decoder::find(codec)?;
    let method = ffmpeg_next::ffi::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32;
    DEVICES.iter().find_map(|&kind| {
        // SAFETY: the configurations are static and read until the first null one.
        let supported = (0..).map_while(|i| unsafe {
            ffmpeg_next::ffi::avcodec_get_hw_config(decoder.as_ptr(), i).as_ref()
        });
        if !supported
            .into_iter()
            .any(|config| config.device_type == kind && config.methods & method != 0)
        {
            return None;
        }
        let mut device = std::ptr::null_mut();
        // SAFETY: on success `device` is a new reference that the caller owns.
        let opened = unsafe {
            ffmpeg_next::ffi::av_hwdevice_ctx_create(
                &mut device,
                kind,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            )
        };
        if opened < 0 || device.is_null() {
            return None;
        }
        // SAFETY: FFmpeg's names of device types are static strings.
        let name =
            unsafe { std::ffi::CStr::from_ptr(ffmpeg_next::ffi::av_hwdevice_get_type_name(kind)) };
        Some((device, name.to_str().unwrap_or("gpu")))
    })
}

/// `value` in time base `from`, in time base `to`, rounded to the nearest.
fn rescale(value: i64, from: Rational, to: Rational) -> i64 {
    let num = i128::from(value) * i128::from(from.numerator()) * i128::from(to.denominator());
    let den = i128::from(from.denominator()) * i128::from(to.numerator());
    i64::try_from((num + den / 2).div_euclid(den)).unwrap_or(i64::MAX)
}

/// The timestamp, in the stream's time base, at which frame `index` starts.
fn index_to_ticks(index: u64, time_base: Rational, rate: Rate) -> i64 {
    let num = i128::from(index) * i128::from(rate.den) * i128::from(time_base.denominator());
    let den = i128::from(rate.num) * i128::from(time_base.numerator());
    i64::try_from(num / den).unwrap_or(i64::MAX)
}

/// The frame a timestamp (from the stream's start) falls in, rounded to the nearest so a time base
/// that cannot hold the frame duration exactly (1/90000 at 29.97) still lands on the right frame.
fn ticks_to_index(ticks: i64, time_base: Rational, rate: Rate) -> u64 {
    let num = i128::from(ticks) * i128::from(time_base.numerator()) * i128::from(rate.num);
    let den = i128::from(time_base.denominator()) * i128::from(rate.den);
    u64::try_from((num + den / 2) / den).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_and_indexes_should_round_trip_at_common_time_bases() {
        let cases = [
            (Rational::new(1, 25), Rate::new(25, 1)),
            (Rational::new(1, 12_800), Rate::new(25, 1)),
            (Rational::new(1, 30_000), Rate::new(30_000, 1_001)),
            (Rational::new(1, 90_000), Rate::new(30_000, 1_001)),
            (Rational::new(1, 24_000), Rate::new(24_000, 1_001)),
        ];
        for (time_base, rate) in cases {
            for index in [0, 1, 2, 29, 1_000, 107_892] {
                let ticks = index_to_ticks(index, time_base, rate);
                assert_eq!(
                    ticks_to_index(ticks, time_base, rate),
                    index,
                    "{time_base} {index}"
                );
            }
        }
    }

    #[test]
    fn rescale_should_round_to_the_nearest() {
        assert_eq!(
            rescale(1_001, Rational::new(1, 30_000), Rational::new(1, 1_000_000)),
            33_367
        );
        assert_eq!(
            rescale(1, Rational::new(1, 25), Rational::new(1, 1_000_000)),
            40_000
        );
    }
}
