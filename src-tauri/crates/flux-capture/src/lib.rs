//! Live video inputs: cameras and capture cards, read through FFmpeg's libavdevice (`dshow` on
//! Windows, `avfoundation` on macOS, `v4l2` on Linux). A [`Capture`] opens one device on a thread
//! of its own and keeps only the newest picture, as `flux-video`'s player does for a file.
//!
//! A device that is missing is a normal state, never an error: [`devices`] lists what it finds
//! (nothing when it finds nothing), and a [`Capture`] of a device that is not there keeps trying to
//! open it, so one plugged in later starts showing. The thread never logs: it counts in
//! [`CaptureStats`] for the `flux-log` lane to write.

use std::ffi::{CStr, c_char};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use ffmpeg_next::Format;
use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::format::{self, Pixel};
use ffmpeg_next::media::Type;
use ffmpeg_next::software::scaling::{Context as Scaler, Flags};
use ffmpeg_next::{device, ffi, frame};

/// The widest a live picture is kept: a wider one is scaled down, which keeps copying and
/// uploading it cheap.
const MAX_WIDTH: u32 = 1280;
/// How long to wait before trying again to open a device that is not there.
const RETRY: Duration = Duration::from_secs(2);

/// The libavdevice input that reads video devices on this system.
#[cfg(target_os = "windows")]
const INPUT: &str = "dshow";
#[cfg(target_os = "macos")]
const INPUT: &str = "avfoundation";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const INPUT: &str = "v4l2";

/// A video input device found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// What opens it, and what a clip stores.
    pub name: String,
    /// What to show a person; the name when the system gives nothing better.
    pub label: String,
}

/// One live picture, packed RGBA (rows without padding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Counts up with each picture, so a caller can tell a new one from the one it has.
    pub index: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What a capture thread did, counted for the `flux-log` lane to write.
#[derive(Debug, Default)]
pub struct CaptureStats {
    /// Pictures handed to `latest`.
    pub frames: AtomicU64,
    /// Times the device was opened.
    pub opened: AtomicU64,
    /// Times the device went away or failed to open.
    pub lost: AtomicU64,
}

/// The video input devices this machine has: empty when there are none, or the system cannot say.
pub fn devices() -> Vec<Device> {
    if ffmpeg_next::init().is_err() {
        return Vec::new();
    }
    let Some(Format::Input(input)) = input_format() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut list: *mut ffi::AVDeviceInfoList = ptr::null_mut();
    // SAFETY: `input` is a valid libavdevice input. FFmpeg allocates the list; its entries (and
    // their nul-terminated strings) stay valid until it is freed below, after they are copied.
    unsafe {
        let listed = ffi::avdevice_list_input_sources(
            input.as_ptr(),
            ptr::null(),
            ptr::null_mut(),
            &mut list,
        );
        if listed >= 0 && !list.is_null() {
            for at in 0..(*list).nb_devices.max(0) as usize {
                let info = *(*list).devices.add(at);
                if info.is_null() || !is_video(&*info) {
                    continue;
                }
                let name = text((*info).device_name);
                if name.is_empty() {
                    continue;
                }
                let description = text((*info).device_description);
                let label = if description.is_empty() {
                    name.clone()
                } else {
                    description
                };
                found.push(Device { name, label });
            }
        }
        ffi::avdevice_free_list_devices(&mut list);
    }
    // FFmpeg's avfoundation input cannot list devices, but it opens `default` as the system's
    // default camera. The UI words it (`label` is the name itself, which it knows).
    if found.is_empty() && INPUT == "avfoundation" {
        found.push(Device {
            name: DEFAULT_CAMERA.into(),
            label: DEFAULT_CAMERA.into(),
        });
    }
    found
}

/// The name of the system's default camera, for inputs that cannot list their devices.
pub const DEFAULT_CAMERA: &str = "default";

/// A string FFmpeg owns, copied.
///
/// # Safety
/// `p` must be null or a nul-terminated string.
unsafe fn text(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: the caller's promise.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

/// Whether `info` carries video (a device that does not say which media it has is taken to).
///
/// # Safety
/// `info.media_types` must be null or point to `nb_media_types` values.
unsafe fn is_video(info: &ffi::AVDeviceInfo) -> bool {
    if info.media_types.is_null() || info.nb_media_types <= 0 {
        return true;
    }
    // SAFETY: the caller's promise.
    let types =
        unsafe { std::slice::from_raw_parts(info.media_types, info.nb_media_types as usize) };
    types.contains(&ffi::AVMediaType::AVMEDIA_TYPE_VIDEO)
}

fn input_format() -> Option<Format> {
    device::input::video().find(|format| format.name() == INPUT)
}

/// What opening `device` is given: dshow wants `video=<name>`, the others the name itself.
fn url(device: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("video={device}")
    } else if INPUT == "avfoundation" {
        // avfoundation reads "<video>:<audio>"; `none` leaves the audio out.
        format!("{device}:none")
    } else {
        device.to_owned()
    }
}

/// An open live input: a thread reads the device and keeps the newest picture. Dropping it stops
/// the thread, which closes the device.
pub struct Capture {
    latest: Arc<Mutex<Option<Arc<Frame>>>>,
    connected: Arc<AtomicBool>,
    stats: Arc<CaptureStats>,
    /// Dropping this tells the thread to stop.
    _stop: mpsc::Sender<()>,
}

impl Capture {
    /// Starts reading `device`. It returns at once: the device opens on the thread, and again
    /// whenever it goes away.
    pub fn start(device: &str) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let latest = Arc::new(Mutex::new(None));
        let connected = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(CaptureStats::default());
        let spawned = thread::Builder::new().name("flux-capture".into()).spawn({
            let latest = Arc::clone(&latest);
            let connected = Arc::clone(&connected);
            let stats = Arc::clone(&stats);
            let device = device.to_owned();
            move || {
                loop {
                    if read(&device, &stopped, &latest, &connected, &stats) {
                        break;
                    }
                    connected.store(false, Ordering::Relaxed);
                    stats.lost.fetch_add(1, Ordering::Relaxed);
                    if !matches!(stopped.recv_timeout(RETRY), Err(RecvTimeoutError::Timeout)) {
                        break;
                    }
                }
            }
        });
        // No thread is the same as a device that is not there: it shows nothing.
        drop(spawned);
        Self {
            latest,
            connected,
            stats,
            _stop: stop,
        }
    }

    /// The newest picture, `None` until the first one arrives.
    pub fn latest(&self) -> Option<Arc<Frame>> {
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Whether the device is open now.
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn stats(&self) -> &CaptureStats {
        &self.stats
    }
}

/// Opens `device` and hands its pictures to `latest` until it fails (`false`) or `stopped` closes
/// (`true`).
fn read(
    device: &str,
    stopped: &mpsc::Receiver<()>,
    latest: &Mutex<Option<Arc<Frame>>>,
    connected: &AtomicBool,
    stats: &CaptureStats,
) -> bool {
    let Ok(mut input) = open(device) else {
        return false;
    };
    let Some(stream) = input.streams().best(Type::Video) else {
        return false;
    };
    let index = stream.index();
    let Ok(mut decoder) = CodecContext::from_parameters(stream.parameters())
        .and_then(|context| context.decoder().video())
    else {
        return false;
    };
    stats.opened.fetch_add(1, Ordering::Relaxed);
    connected.store(true, Ordering::Relaxed);
    let mut scaler: Option<Scaler> = None;
    let mut decoded = frame::Video::empty();
    for (stream, packet) in input.packets() {
        if matches!(stopped.try_recv(), Err(TryRecvError::Disconnected)) {
            return true;
        }
        if stream.index() != index || decoder.send_packet(&packet).is_err() {
            continue;
        }
        while decoder.receive_frame(&mut decoded).is_ok() {
            // The count never goes back, not even when the device is opened again.
            if let Some(picture) = rgba(&decoded, &mut scaler, stats.frames.load(Ordering::Relaxed))
            {
                *latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(picture));
                stats.frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    // The packets ended: the device was unplugged or failed.
    matches!(stopped.try_recv(), Err(TryRecvError::Disconnected))
}

fn open(device: &str) -> Result<format::context::Input, ffmpeg_next::Error> {
    ffmpeg_next::init()?;
    let input = input_format().ok_or(ffmpeg_next::Error::DemuxerNotFound)?;
    match format::open(&url(device), &input)? {
        format::context::Context::Input(input) => Ok(input),
        format::context::Context::Output(_) => Err(ffmpeg_next::Error::DemuxerNotFound),
    }
}

/// `picture` as packed RGBA, no wider than [`MAX_WIDTH`].
fn rgba(picture: &frame::Video, scaler: &mut Option<Scaler>, index: u64) -> Option<Frame> {
    let (source_w, source_h) = (picture.width(), picture.height());
    if source_w == 0 || source_h == 0 {
        return None;
    }
    let width = source_w.min(MAX_WIDTH);
    let height = ((u64::from(source_h) * u64::from(width) / u64::from(source_w)) as u32).max(1);
    let scaler = match scaler {
        Some(s)
            if s.input().width == source_w
                && s.input().height == source_h
                && s.input().format == picture.format()
                && (s.output().width, s.output().height) == (width, height) =>
        {
            s
        }
        slot => slot.insert(
            Scaler::get(
                picture.format(),
                source_w,
                source_h,
                Pixel::RGBA,
                width,
                height,
                Flags::BILINEAR,
            )
            .ok()?,
        ),
    };
    let mut out = frame::Video::empty();
    scaler.run(picture, &mut out).ok()?;
    let row = width as usize * 4;
    let stride = out.stride(0);
    let data = out.data(0);
    let mut packed = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        packed.extend_from_slice(data.get(y * stride..y * stride + row)?);
    }
    Some(Frame {
        index,
        width,
        height,
        rgba: packed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_never_fails_without_hardware() {
        // A runner has no camera: the list is whatever it finds, maybe nothing.
        let _ = devices();
    }

    #[test]
    fn a_missing_device_is_a_capture_with_no_picture() {
        let capture = Capture::start("/dev/flux-no-such-camera");
        assert!(capture.latest().is_none());
        assert!(!capture.connected());
        // Dropping stops its thread.
        drop(capture);
    }
}
