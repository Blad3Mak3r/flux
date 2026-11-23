# Video decoding

Video tracks ([project format](fxtc-format.md), version 13) are decoded by the `flux-video` crate
(`src-tauri/crates/flux-video/`) through **FFmpeg**. This page records why, what the crate does, and what building
FLUX needs now that it links FFmpeg. How to use video in a show is in [Video and projection mapping](video-output.md).

## Why FFmpeg

| Need | How FFmpeg meets it |
| --- | --- |
| MP4 first (H.264, HEVC) | Native decoders, with hardware decoding available on every platform (VideoToolbox, D3D11VA, VA-API). |
| Codecs made for shows (HAP, ProRes) | Decoded by the same library, so a show-ready file needs no second backend. |
| Frame-exact seeking | Seek to the keyframe before a timestamp, then decode up to the frame: one code path for every codec. |
| Three platforms | One API on Windows, macOS and Linux, instead of AVFoundation, Media Foundation and GStreamer side by side. |
| Licence | FFmpeg is LGPL-2.1-or-later (GPL when built with GPL parts). Both can be combined with FLUX's GPL-3.0-or-later. |

The alternatives weighed were GStreamer (a large runtime to bundle and harder to ask for "frame N"), the native
frameworks of each system (three backends to maintain, and no HAP), libmpv (a player with a clock of its own, which
contradicts "the engine clock decides") and pure-Rust decoders (H.264 only, software only).

Decoding H.264 and HEVC in software may need a patent licence in some countries when the decoder is distributed.
Hardware decoding through the system's own decoders keeps that with the platform vendor; the bundling step will
decide which decoders the shipped FFmpeg contains.

The Rust bindings are [`ffmpeg-next`](https://crates.io/crates/ffmpeg-next), with only the `format`, `codec` and
`software-scaling` features (demuxing, decoding and the conversion to RGBA).

## What the crate does

- **`source_frame`**: which frame of a video a clip shows at a timeline position. It is a pure function of the
  position, the clip's start, trim and length, the timeline rate and the video's own rate, so a seek and playback
  land on the same frame and the picture never follows a timer of its own. The arithmetic is on integers: equal
  rates map frame for frame, 29.97 included; at different rates the frame shown is the last one that started.
- **`VideoFile`**: an open file that hands out any frame by index as packed RGBA. A frame up to 48 frames ahead of
  the last one decoded is reached by decoding on; anything else seeks to the keyframe before it and decodes up to
  it. A gap in the timestamps shows the frame before it.
  `VideoFile::open_accelerated` decodes on the GPU when the computer has a decoder for the codec: it tries, in order,
  D3D11VA then DXVA2 on Windows, VideoToolbox on macOS, and VA-API then CUDA on Linux, and the first device that opens
  and that the codec supports is handed to FFmpeg (`hw_device_ctx`, with 4 extra surfaces for the frames kept while one
  is chosen). Each frame is copied from the GPU (`av_hwframe_transfer_data`) and converted to RGBA as before. When
  FFmpeg cannot take the stream on the GPU (a profile the GPU lacks, such as 4:2:2 or lossless) it goes on on the CPU by
  itself, and a decode error on the GPU reopens the decoder on the CPU and tries the frame again; either way the file
  stays on the CPU from then on. `decoding()` says which it is. Probing and thumbnails use `open`, on the CPU.
- **`VideoPlayer`**: one `VideoFile` on a thread of its own. The caller requests the frame the clock is at and
  reads the newest decoded one; only the newest request is decoded, so a slow decode drops frames instead of
  lagging behind the clock. The thread never logs: it counts decoded, skipped and failed requests in atomics
  (`PlayerStats`) for the `flux-log` lane (see [Diagnostics](diagnostics.md)). Players decode with
  `open_accelerated`; `VideoDeck::decoding` reports the GPU device when any open clip uses one. The Video page of the
  preferences shows it, and the main screen's output writes it to the log when it changes.

- **`layers`**: what the video tracks of a timeline show for a `Cue` (`Black` when stopped, `At(frame)` when playing
  or paused), topmost first: a track higher in the timeline covers the ones below, a muted track shows nothing, and
  where clips of one track overlap the one that starts later wins, with the one under it too while it fades. Each layer
  carries its clip's fit and its opacity. `upcoming` lists the clips that start soon.
- **`VideoDeck`**: turns positions into pictures. Each call opens a player for every clip showing or starting within
  `PRELOAD_FRAMES` (60), closes the others, asks each player for the frame its position maps to, and returns the
  layers with the newest decoded frame of each. It has no timer: the output draws a picture, reads the engine's
  position and calls it, so seek, stop and loops follow the clock by construction. Files that fail to open are
  listed for Show check.

The output window reads the engine like this, then draws what the deck returns:

```rust
let snapshot = shared_clock.load();
let cue = match snapshot.state {
    TransportState::Stopped => Cue::Black,
    _ => Cue::At(clock.samples_to_frames(snapshot.estimated_position(shared_clock.now(), clock))),
};
let layers = deck.show(active_timeline, cue);
```

The following steps of issue #106 add that output window and the timeline clips.

## Importing

**File > Import > Video** (`project_import_video`) opens the file with FFmpeg first, so a file it cannot read is
refused with the reason, and then adds it on a new video track of the active timeline with a clip at the playhead.
The file is copied into the project's `video/` folder when the project is saved, like audio.

The dialog offers MP4, M4V, MOV and WebM. WebM (VP8, VP9 or AV1 in Matroska) uses FFmpeg's own VP8/VP9 decoders and
dav1d for AV1. A WebM file has no frame count per stream, so the count comes from the container's duration. A VP8/VP9
WebM with an alpha track (Matroska's `alpha_mode` tag) is decoded with libvpx (`libvpx`, `libvpx-vp9`) instead, the
only decoder that reads that track; it decodes on the CPU. Formats with an alpha channel of their own (ProRes 4444, HAP
Alpha, PNG, QuickTime Animation) keep it through the conversion to RGBA. `VideoInfo::alpha` and `Frame::alpha` say so:
the output blends such a picture over the layers below by its alpha, never takes it as hiding them, and its thumbnails
show it over black.

## Converting to HAP

HAP compresses every frame on its own, as media servers expect: a seek costs one frame and decoding is light enough
for several layers at full size. The video clip inspector shows the file's codec and, for any other, **Convert to
HAP** (`video_convert_hap`, offered when `video_hap_available` says this FFmpeg has the `hap` encoder, which needs
snappy). `flux_video::convert_to_hap` reads the frames with `VideoFile`, as playback does, so frame `n` of the copy is
frame `n` of the source at the same rate, and writes them into a QuickTime file as HAP, or HAP Alpha when the source
has alpha, at the source's size rounded up to a multiple of 4 (what HAP's textures need). The sound goes beside them
as 16-bit PCM, decoded the way the mixer hears it, so it stays in step. Progress goes out as
`video-convert-progress` events; `video_convert_cancel` stops it and removes the partial file. One conversion runs
at a time.

The copy is written beside the imported file as `<name> (HAP).mov` (`(HAP 2)` and on when that exists), or into
`converted/` in the video cache for a video kept inside the project or in a folder that cannot be written. When it is
done, `project_replace_video` (`Session::replace_video`) makes it a new asset in the place and under the name of the
old one and moves every clip of every timeline onto it, keeping their place, trim, fit and fades: one step of undo,
which brings the old file back. A save keeps only the asset in use.

FFmpeg's HAP decoder still hands over RGBA, so the frames are uploaded to the graphics card as for any codec.

## Editing clips

A video clip is edited like an audio clip: drag it to move it, drag its edges to trim it, split it at
the playhead and delete it. The window only says which edit and where; `video_clip_edit` applies it
in the backend (`flux-project/src/video_clips.rs`), with the length of the video read by FFmpeg. A
clip's `offset` and `length` count frames at the timeline's rate, as for audio.

The clip shows thumbnails of the video, one per second or further apart for a long video (at most
600). `video_thumbnails` decodes them once at 54 pixels high into one JPEG strip and keeps it in the
app's cache folder (`video/thumbs/<asset id>.jpg`), so opening the project again reads them back.

## Sound

The sound of a video plays with its clips, like an audio clip at 0 dB with no fades. A video
track has a fader, a level meter and an output in the routing, as an audio track does. Pulling the fader
down silences the sound and leaves the picture; muting the track takes both away. FFmpeg decodes the
video's main audio stream once (`flux_video::audio_wav`), and the audio crate converts it to the
engine's rate; it is kept in the audio cache beside the project's audio files. Only the first two
channels play.

The clip draws the waveform of that sound under its thumbnails and shows the tempo found in it, as an
audio clip does (`video_sound`). The tempo is read again each time the project opens; it is not
saved with the video.

## Output window

The output has one or more **screens**, each a window on a display of its own with its own slices (see Mapping). The
main screen always exists; **Add screen** in **Preferences > Outputs > Video** adds up to eight, which can be renamed and
removed. The screens belong to the project (`output.screens`); which display each opens on is a local preference
(`videoDisplays`, by screen id, like the audio devices; the older single `videoDisplay` is read as the main screen's). On
a display that has a FLUX window on it, a screen opens as a window that can be moved aside, so one screen is never
covered; on any other display it fills the screen without decorations.

`src-tauri/src/video_output.rs` owns the windows. A thread (`flux-video-screens`) follows the project and opens,
renames or closes a window when a screen is added, renamed or removed or another project opens. Each window's surface is made on the main thread (macOS requires it), and a
render thread (`flux-video-output`, one per window, all sharing one `VideoDeck`, so a clip is decoded once however many screens show it) starts the GPU device and then, once per display refresh, reads the transport
(`PositionReader::timeline_frame`), asks the `VideoDeck` for the layers at that frame and draws them with
`flux-render`, from the top one down to the first opaque one that fills the composition (at most `MAX_LAYERS`, 4), one
over another with alpha blending. Each frame is placed in the composition (`output.composition`, else the first video's
size) as its clip's `fit` says (`flux_render::placement`) and covers what is below by its layer's opacity, which the
clip's `fadeIn` and `fadeOut` ramp from 0 on its first and last frame (`flux_video::opacity`); outside its place the
layers below show, and black under all of them. While the top clip of a track fades, the clip it overlaps on that track
is drawn under it, which cross-fades them. With no slices the
composition is fitted to the window keeping its shape, with black around it. It is black while the
transport is stopped, before a clip and after it. A GPU error is kept for the setup page to show and never panics.

## Mapping

Below the display, **Mapping** cuts the picture into slices for the selected screen, as the advanced output of a media server does. Each slice
is a rectangle of the picture (dragged and resized on the left, *Picture*) placed on the screen by its four corners
(dragged on the right, *Screen*); moving a corner leans the slice to fit a surface seen at an angle, and dragging the
shape moves it whole. Below the stages, the selected slice's rectangle and corners can also be typed in pixels: the picture is
measured in the size of the composition and the corners in the chosen display's
size, and the backend turns the pixels into fractions (`setInputValue`, `setCornerValue`). A slice can be renamed, hidden without losing its place, or have its corners put back where its
rectangle is. **Test pattern** shows a test card wherever there is no picture, to line the slices up on the real
screen: a grid of tinted cells, the diagonals and two circles that show how a slice leans, colour bars and a grey ramp,
coloured corners (red top left, green top right, blue bottom right, yellow bottom left), the picture's size and
the time of day, each slice outlined in a colour of its own with its name, size and position in pixels, and a white bar that sweeps
across every three seconds, so each slice shows which way its part of the picture runs
(`flux-render/src/pattern.rs`). No slices is the plain
output: the whole picture fitted to the screen.

**Soft edge** fades a slice to black over a percentage of its width or height from each side, so two projectors
whose pictures overlap blend into one; **Curve** shapes the fade (1 dims the light evenly, more keeps it brighter for
longer, less darker). **Add mask** puts a square on the screen whose points are dragged (or the whole shape), with
**Add point** after the selected point and **Remove point** down to three. A mask blacks out its shape over every slice
of its screen, or, inverted, everything but its shape; up to 8 masks of up to 32 points. `flux-render` does both in the
fragment shader: the soft edge from each fragment's place in its slice, and the masks with an even-odd test against
their outlines, passed in a uniform (`video_mask_edit` edits them).

The slices belong to the project (`output.screens`, see [the format](fxtc-format.md)), so they travel with the show,
while the display stays a local preference. Every number is a fraction of a width or height, so a mapping keeps its
shape on another resolution. `src-tauri/crates/flux-project/src/mapping.rs` owns the operations (`video_mapping_edit`):
it clamps a rectangle inside the picture and a corner to one screen past each edge, and each drag reaches it once, on
release, so it is one undo step. `flux-render` draws each slice as two triangles with perspective-correct texture
coordinates (each corner weighted by its distance to where the diagonals cross), so a leaning slice does not bend
along its diagonal.

Show check reports a show with video clips whose screens are all off (a warning) or a screen whose display is not
connected (an error).

## Building

FFmpeg's development files (headers and libraries) must be installed to build the workspace:

| Platform | Install |
| --- | --- |
| Ubuntu / Debian | `sudo apt install libavformat-dev libavcodec-dev libavutil-dev libswscale-dev` (plus `clang` and `pkg-config`) |
| macOS | `brew install ffmpeg pkg-config` |
| Windows | `pwsh scripts/fetch-ffmpeg.ps1` downloads the shared LGPL build from [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases) into `src-tauri/ffmpeg`. Set `FFMPEG_DIR` to that folder, and add its `bin` to `PATH` for `cargo test`. The app build copies the DLLs next to the executable. |

The bindings are generated with bindgen, which needs libclang (CI already sets `LIBCLANG_PATH` on Windows for the
ASIO bindings). CI installs FFmpeg in `.github/actions/linux-deps`, `.github/actions/macos-deps` and
`.github/actions/windows-deps`.

## FFmpeg in the installers

FLUX always ships the FFmpeg it was built against, so a user installs nothing else:

| Installer | How FFmpeg comes with it |
| --- | --- |
| Windows (NSIS) | `src-tauri/tauri.windows.conf.json` adds `avcodec`, `avformat`, `avutil`, `swresample` and `swscale` from `src-tauri/ffmpeg/bin` next to `flux-timecode.exe`, and FFmpeg's `LICENSE.txt` under `ffmpeg/`. |
| Linux AppImage | linuxdeploy bundles the FFmpeg libraries the binary links, like its other libraries. |
| Linux deb / rpm | The package depends on the runner's FFmpeg libraries (`libavformat60` and the rest; for rpm, their sonames), which the package manager installs. The release workflow stops if the runner's FFmpeg major versions no longer match `tauri.conf.json`. |
| macOS | Not released yet. A macOS installer will need FFmpeg's dylibs inside the app bundle, since Homebrew's are not portable. |

The shipped build is LGPL, so FFmpeg can be replaced by the user, as the licence requires.

## Tests

The tests need no GPU, display or hardware decoder; on a computer with one, the accelerated decode is checked against
the CPU's frames. Three fixtures in `tests/fixtures/` (64x36, 50 frames, each a
lighter grey than the one before, with a keyframe every 12 frames: lossless H.264 in MP4 at 25 and 29.97 fps, and
lossless VP9 in WebM at 25 fps) check that
every frame decodes as a different picture and that seeking in any order gives the same frame as playing through.
`source_frame` is tested at all four frame rates. To make a fixture again:

```bash
python3 -c "import sys; [sys.stdout.buffer.write(bytes([20+4*i])*64*36) for i in range(50)]" \
  | ffmpeg -f rawvideo -pix_fmt gray -s 64x36 -r 25 -i - -c:v libx264 -qp 0 -pix_fmt yuv420p \
      -g 12 -bf 2 -movflags +faststart -map_metadata -1 -fflags +bitexact gray25.mp4
```

For the WebM one, replace the encoder options with
`-c:v libvpx-vp9 -lossless 1 -pix_fmt yuv420p -g 12 -map_metadata -1 -fflags +bitexact gray25.webm`.

`tests/hap.rs` converts the fixtures and reads the copies back: the same size, rate, frame count and grey levels,
HAP Alpha for `alpha25.webm`, the sound of `tone.mp4` and `tone.webm` sample for sample, and nothing left behind when
cancelled. It is skipped where FFmpeg has no HAP encoder.

`alpha25.webm` (10 frames of 64x36, the left half opaque red and the right half transparent) checks that a WebM alpha
track is kept:

```bash
python3 -c "import sys; r=bytes([255,0,0,255])*32+bytes([0,0,255,0])*32; sys.stdout.buffer.write(r*36*10)" \
  | ffmpeg -f rawvideo -pix_fmt rgba -s 64x36 -r 25 -i - -c:v libvpx-vp9 -pix_fmt yuva420p -lossless 1 \
      -auto-alt-ref 0 -map_metadata -1 -fflags +bitexact alpha25.webm
```
