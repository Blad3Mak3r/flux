# Video decoding

Video tracks ([project format](fxtc-format.md), version 13) are decoded by the `flux-video` crate
(`src-tauri/crates/flux-video/`) through **FFmpeg**. This page records why, what the crate does, and what building
FLUX needs now that it links FFmpeg.

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
- **`VideoPlayer`**: one `VideoFile` on a thread of its own. The caller requests the frame the clock is at and
  reads the newest decoded one; only the newest request is decoded, so a slow decode drops frames instead of
  lagging behind the clock. The thread never logs: it counts decoded, skipped and failed requests in atomics
  (`PlayerStats`) for the `flux-log` lane (see [Diagnostics](diagnostics.md)).

- **`layers`**: what the video tracks of a timeline show for a `Cue` (`Black` when stopped, `At(frame)` when playing
  or paused), topmost first: a track higher in the timeline covers the ones below, a muted track shows nothing, and
  where clips of one track overlap the one that starts later wins. `upcoming` lists the clips that start soon.
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
dav1d for AV1. A WebM file has no frame count per stream, so the count comes from the container's duration. An alpha
channel in VP8/VP9 WebM is not decoded yet: those frames show as opaque.

## Output window

**Preferences > Outputs > Video** chooses the display the output opens on; the choice is a local preference
(`videoDisplay`), like the audio devices. On a display that has a FLUX window on it, the output opens as a window that
can be moved aside, so one screen is never covered; on any other display it fills the screen without decorations.

`src-tauri/src/video_output.rs` owns the window. Its surface is made on the main thread (macOS requires it), and a
render thread (`flux-video-output`) starts the GPU device and then, once per display refresh, reads the transport
(`PositionReader::timeline_frame`), asks the `VideoDeck` for the layers at that frame and draws the top one with
`flux-render`. The picture is fitted to the window keeping its shape, with black around it. It is black while the
transport is stopped, before a clip and after it. A GPU error is kept for the setup page to show and never panics.

Show check reports a show with video clips whose output is off (a warning) or whose display is not connected (an
error).

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

The tests need no GPU, display or hardware decoder. Three fixtures in `tests/fixtures/` (64x36, 50 frames, each a
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
