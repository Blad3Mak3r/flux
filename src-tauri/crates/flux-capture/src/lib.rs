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
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread;
use std::time::Duration;

use ffmpeg_next::codec::Flags as CodecFlags;
use ffmpeg_next::codec::context::Context as CodecContext;
use ffmpeg_next::format::{self, Pixel};
use ffmpeg_next::media::Type;
use ffmpeg_next::software::scaling::{Context as Scaler, Flags};
use ffmpeg_next::{Dictionary, Format};
use ffmpeg_next::{device, ffi, frame};

/// The device is held by another program (`EBUSY`, the same number on every system we run on).
const EBUSY: i32 = 16;
/// The error DirectShow gives for most failures to run a device (`EIO`).
const EIO: i32 = 5;
/// DirectShow's real-time buffer, in bytes: the default is too small for 1080p60 and drops frames.
const RTBUFSIZE: &str = "268435456";
/// What opening reads to find the stream, kept small so a device opens fast.
const PROBESIZE: &str = "4194304";
/// Microseconds the opening analyses the stream for.
const ANALYZE: &str = "500000";
/// How long to wait before trying again to open a device that is not there.
const RETRY: Duration = Duration::from_secs(2);

/// The kinds of libavdevice input Flux reads, which differ in the options they take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // each system builds one; the tests use all
enum Os {
    Dshow,
    AvFoundation,
    V4l2,
}

#[cfg(target_os = "windows")]
const OS: Os = Os::Dshow;
#[cfg(target_os = "macos")]
const OS: Os = Os::AvFoundation;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const OS: Os = Os::V4l2;

/// The format asked of a device; `None` leaves it to the device.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Request {
    /// Width and height in pixels.
    pub size: Option<(u32, u32)>,
    /// The frame rate as FFmpeg writes it (`"59.94"`).
    pub rate: Option<&'static str>,
}

/// The options opening a device on `os` is given, for `request`.
fn options(os: Os, request: Request) -> Vec<(&'static str, String)> {
    let mut list = Vec::new();
    if let Some((w, h)) = request.size {
        list.push(("video_size", format!("{w}x{h}")));
    }
    if let Some(rate) = request.rate {
        list.push(("framerate", rate.to_owned()));
    }
    if os == Os::Dshow {
        list.push(("rtbufsize", RTBUFSIZE.to_owned()));
    }
    list.push(("fflags", "nobuffer".to_owned()));
    list.push(("probesize", PROBESIZE.to_owned()));
    list.push(("analyzeduration", ANALYZE.to_owned()));
    list
}

/// Whether failing to open with `errno` means another program holds a device that is there.
/// DirectShow answers `EIO` to that and to a device that is not there, so it counts only for a
/// device the system lists (`listed`).
fn is_busy(os: Os, errno: i32, listed: impl FnOnce() -> bool) -> bool {
    errno == EBUSY || (os == Os::Dshow && errno == EIO && listed())
}

/// The size a `source` picture is scaled to: inside `fit`, keeping its shape, never larger than it
/// is. A `fit` with a zero side leaves it as it is.
fn fitted(source: (u32, u32), fit: (u32, u32)) -> (u32, u32) {
    let ((w, h), (fw, fh)) = (source, fit);
    if fw == 0 || fh == 0 || (w <= fw && h <= fh) {
        return (w, h);
    }
    // The smaller of the two ratios, in integers: w * fh against h * fw.
    if u64::from(w) * u64::from(fh) >= u64::from(h) * u64::from(fw) {
        let h = (u64::from(h) * u64::from(fw) / u64::from(w)) as u32;
        (fw, h.max(1))
    } else {
        let w = (u64::from(w) * u64::from(fh) / u64::from(h)) as u32;
        (w.max(1), fh)
    }
}

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

/// Whether a device is open, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Not open: it is not there, or not yet.
    NotConnected,
    Connected,
    /// It is there but another program holds it.
    Busy,
}

/// What a [`Capture`] says about its device, for the interface to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub state: State,
    /// The device refused the format asked, so it is open as `auto`.
    pub refused: bool,
}

/// What a capture thread and its owner share.
#[derive(Debug, Default)]
struct Shared {
    /// A [`State`] as a number: 0 not connected, 1 connected, 2 busy.
    state: AtomicU8,
    refused: AtomicBool,
    /// The composition's width in the high half and its height in the low: pictures are scaled
    /// to fit inside it.
    fit: AtomicU64,
}

impl Shared {
    fn set(&self, state: State) {
        self.state.store(state as u8, Ordering::Relaxed);
    }

    fn status(&self) -> Status {
        let state = match self.state.load(Ordering::Relaxed) {
            1 => State::Connected,
            2 => State::Busy,
            _ => State::NotConnected,
        };
        Status {
            state,
            refused: self.refused.load(Ordering::Relaxed),
        }
    }
}

/// The open captures by device name, so the interface can ask about one.
static OPEN: Mutex<Vec<(String, Weak<Shared>)>> = Mutex::new(Vec::new());

/// The status of the capture of `device`, `None` when none is open (no clip shows it).
pub fn status(device: &str) -> Option<Status> {
    let mut open = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
    open.retain(|(_, shared)| shared.strong_count() > 0);
    open.iter()
        .find(|(name, _)| name == device)
        .and_then(|(_, shared)| shared.upgrade())
        .map(|shared| shared.status())
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
    shared: Arc<Shared>,
    stats: Arc<CaptureStats>,
    /// Dropping this tells the thread to stop.
    _stop: mpsc::Sender<()>,
}

impl Capture {
    /// Starts reading `device` in the format `request`, its pictures scaled to fit inside `fit`
    /// (see [`Capture::fit`]). It returns at once: the device opens on the thread, and again
    /// whenever it goes away.
    pub fn start(device: &str, request: Request, fit: (u32, u32)) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let latest = Arc::new(Mutex::new(None));
        let shared = Arc::new(Shared::default());
        let stats = Arc::new(CaptureStats::default());
        let spawned = thread::Builder::new().name("flux-capture".into()).spawn({
            let latest = Arc::clone(&latest);
            let shared = Arc::clone(&shared);
            let stats = Arc::clone(&stats);
            let device = device.to_owned();
            move || {
                loop {
                    if read(&device, request, &stopped, &latest, &shared, &stats) {
                        break;
                    }
                    if shared.status().state == State::Connected {
                        shared.set(State::NotConnected);
                    }
                    stats.lost.fetch_add(1, Ordering::Relaxed);
                    if !matches!(stopped.recv_timeout(RETRY), Err(RecvTimeoutError::Timeout)) {
                        break;
                    }
                }
            }
        });
        // No thread is the same as a device that is not there: it shows nothing.
        drop(spawned);
        let mut open = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
        open.retain(|(_, shared)| shared.strong_count() > 0);
        open.push((device.to_owned(), Arc::downgrade(&shared)));
        drop(open);
        let capture = Self {
            latest,
            shared,
            stats,
            _stop: stop,
        };
        capture.fit(fit);
        capture
    }

    /// Scales the pictures from now on to fit inside `fit` (a side of 0 leaves them as they are).
    pub fn fit(&self, fit: (u32, u32)) {
        let packed = u64::from(fit.0) << 32 | u64::from(fit.1);
        self.shared.fit.store(packed, Ordering::Relaxed);
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
        self.status().state == State::Connected
    }

    /// Whether the device is open, held by another program, or not there, and whether it refused
    /// the format asked.
    pub fn status(&self) -> Status {
        self.shared.status()
    }

    pub fn stats(&self) -> &CaptureStats {
        &self.stats
    }
}

/// Opens `device` and hands its pictures to `latest` until it fails (`false`) or `stopped` closes
/// (`true`).
fn read(
    device: &str,
    request: Request,
    stopped: &mpsc::Receiver<()>,
    latest: &Mutex<Option<Arc<Frame>>>,
    shared: &Shared,
    stats: &CaptureStats,
) -> bool {
    let mut opened = open(device, request);
    if opened.is_err() && request != Request::default() {
        // A device without the format asked opens as it likes, and the interface says so.
        let auto = open(device, Request::default());
        shared.refused.store(auto.is_ok(), Ordering::Relaxed);
        opened = auto;
    }
    let mut input = match opened {
        Ok(input) => input,
        Err(error) => {
            let busy = matches!(error, ffmpeg_next::Error::Other { errno }
                if is_busy(OS, errno, || devices().iter().any(|d| d.name == device)));
            shared.set(if busy {
                State::Busy
            } else {
                State::NotConnected
            });
            return false;
        }
    };
    let Some(stream) = input.streams().best(Type::Video) else {
        return false;
    };
    let index = stream.index();
    let Ok(mut decoder) =
        CodecContext::from_parameters(stream.parameters()).and_then(|mut context| {
            context.set_flags(CodecFlags::LOW_DELAY);
            context.decoder().video()
        })
    else {
        return false;
    };
    stats.opened.fetch_add(1, Ordering::Relaxed);
    shared.set(State::Connected);
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
            if let Some(picture) = rgba(
                &decoded,
                &mut scaler,
                stats.frames.load(Ordering::Relaxed),
                shared.fit.load(Ordering::Relaxed),
            ) {
                *latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(picture));
                stats.frames.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    // The packets ended: the device was unplugged or failed.
    matches!(stopped.try_recv(), Err(TryRecvError::Disconnected))
}

fn open(device: &str, request: Request) -> Result<format::context::Input, ffmpeg_next::Error> {
    ffmpeg_next::init()?;
    let input = input_format().ok_or(ffmpeg_next::Error::DemuxerNotFound)?;
    let mut dictionary = Dictionary::new();
    for (key, value) in options(OS, request) {
        dictionary.set(key, &value);
    }
    match format::open_with(&url(device), &input, dictionary)? {
        format::context::Context::Input(input) => Ok(input),
        format::context::Context::Output(_) => Err(ffmpeg_next::Error::DemuxerNotFound),
    }
}

/// `picture` as packed RGBA, scaled to fit inside `fit` (see [`Shared::fit`]).
fn rgba(
    picture: &frame::Video,
    scaler: &mut Option<Scaler>,
    index: u64,
    fit: u64,
) -> Option<Frame> {
    let (source_w, source_h) = (picture.width(), picture.height());
    if source_w == 0 || source_h == 0 {
        return None;
    }
    let (width, height) = fitted((source_w, source_h), ((fit >> 32) as u32, fit as u32));
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
        let capture = Capture::start("/dev/flux-no-such-camera", Request::default(), (0, 0));
        assert!(capture.latest().is_none());
        assert!(!capture.connected());
        // Dropping stops its thread.
        drop(capture);
    }

    #[test]
    fn options_ask_for_the_format_and_a_quick_open() {
        let asked = Request {
            size: Some((1920, 1080)),
            rate: Some("59.94"),
        };
        let get = |list: &[(&str, String)], key: &str| {
            list.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())
        };
        for os in [Os::Dshow, Os::AvFoundation, Os::V4l2] {
            let list = options(os, asked);
            assert_eq!(get(&list, "video_size").as_deref(), Some("1920x1080"));
            assert_eq!(get(&list, "framerate").as_deref(), Some("59.94"));
            assert_eq!(get(&list, "fflags").as_deref(), Some("nobuffer"));
            assert!(get(&list, "probesize").is_some() && get(&list, "analyzeduration").is_some());
            // Only DirectShow has a real-time buffer.
            assert_eq!(get(&list, "rtbufsize").is_some(), os == Os::Dshow);
            // Auto asks for neither.
            let auto = options(os, Request::default());
            assert!(get(&auto, "video_size").is_none() && get(&auto, "framerate").is_none());
        }
    }

    #[test]
    fn busy_is_ebusy_or_a_listed_device_directshow_cannot_run() {
        assert!(is_busy(Os::V4l2, EBUSY, || false));
        assert!(!is_busy(Os::V4l2, EIO, || true));
        assert!(is_busy(Os::Dshow, EIO, || true));
        assert!(!is_busy(Os::Dshow, EIO, || false));
        assert!(!is_busy(Os::Dshow, 2, || true));
    }

    #[test]
    fn pictures_fit_inside_the_composition_and_never_grow() {
        let hd = (1920, 1080);
        assert_eq!(fitted((3840, 2160), hd), (1920, 1080));
        assert_eq!(fitted((1920, 1080), hd), (1920, 1080));
        assert_eq!(fitted((1280, 720), hd), (1280, 720));
        // Keeps the aspect: a taller picture is limited by the height.
        assert_eq!(fitted((2000, 2000), hd), (1080, 1080));
        assert_eq!(fitted((4000, 1000), hd), (1920, 480));
        // No composition size leaves it alone.
        assert_eq!(fitted((3840, 2160), (0, 0)), (3840, 2160));
    }
}
