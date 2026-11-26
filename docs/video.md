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
`flux-render`, from the top one down to the first opaque one that fills the composition (at most `MAX_LAYERS`, 16), one
over another with alpha blending. Each frame is placed in the composition (`output.composition`, else the first video's
size) as its clip's `fit` says (`flux_render::placement`) and covers what is below by its layer's opacity, which the
clip's `fadeIn` and `fadeOut` ramp from 0 on its first and last frame (`flux_video::opacity`); outside its place the
layers below show, and black under all of them. While the top clip of a track fades, the clip it overlaps on that track
is drawn under it, which cross-fades them. With no slices the
composition is fitted to the window keeping its shape, with black around it. It is black while the
transport is stopped, before a clip and after it. A GPU error is kept for the setup page to show and never panics.

## VJ grid

A VJ project ([project format](fxtc-format.md#vj-grid-version-14)) shows its grid instead of a timeline. Its clips play
on a clock of their own, not the transport's: `flux_video::GridPlay` holds which clip plays on each layer and when it was
launched, and `grid_frame` maps the time since the launch to a frame of the video, looping, stopping after one pass or
going back and forth (`playback`). The output's render threads ask `VideoDeck::show_grid` for the pictures on every
refresh, so a launched clip has a player only while it plays. A layer's picture is its `master` times its `opacity`
times its crossfader side: each side shows fully from its end to the middle and fades out over the other half. `bypass`
hides a layer and a `solo` layer hides every layer that is not soloed. Clip sound is not played yet, so `volume` does
nothing for now.

A layer with a `transition` time keeps drawing the clip it replaces until the new one has faded in: with the `alpha`
effect the new clip fades in over the old one, with `add` the old one fades out while the new one fades in. Launching the
clip that already plays restarts it at once. A clip launched on a layer that plays nothing fades in, and a stopped layer (by hand, by a column without a clip on it, or a piano clip let go) fades out, over the same time. Changing the transition time while a transition runs makes it go on from where it is at the new speed (`GridPlay::retime`, on every command and read of the play state). Each layer is drawn with its `blend` through a fixed GPU blend state
(`flux_render::Blend`): `add`, `subtract`, `multiply`, `screen`, `lighten` and `darken` combine the layer with what is
already drawn, and a layer that is not `alpha` never hides the ones below it.

The app crate's `vj_commands.rs` holds what plays, which is never stored. Its commands answer with what plays now
(`{ playing: [{ layer, clip, seconds, progress, remaining, level, transition, stopping }], column, crossfader }`:
`progress` is where each clip is in its video, `level` how much of it reaches the output, `transition` how far its
layer's transition is while one runs, `stopping` whether the layer is fading out, and `column` the column launched last while its
clips play):

| Command | Arguments | Does |
| --- | --- | --- |
| `vj_launch` | `clip` | Plays the clip on its layer from its first frame, in place of what played there, over the layer's transition; a playing clip starts again. |
| `vj_release` | `clip` | Stops a `piano` clip when it is let go; a `normal` clip keeps playing. |
| `vj_launch_column` | `column` | Launches every clip of the column at once; a deck layer without a clip in it stops and global layers are left as they are. |
| `vj_stop` | `layer?` | Stops the layer, or every layer. |
| `vj_crossfader` | `position` | Moves the crossfader, 0 (side A) to 1 (side B). |
| `vj_play_state` | | Reads what plays. |

A VJ show has a menu bar and keys of its own (`src/vj/menus.ts`): File, Edit, Composition, Deck, Layer, Clip, Output,
View and Help, with no tracks, timelines or transport. Its keys: 1 to 9 launch a column of the deck shown, Enter the
selected clip, Ctrl+PgUp/PgDn change deck, Ctrl+. stops everything, B, S and X bypass, solo and stop the selected layer,
Del removes the selected clip, Ctrl+I imports media and Ctrl+Shift+A opens the output. The timeline's keys (Space, Home,
the arrows, N, S, G, F2, Ctrl+1/2) do nothing in a VJ show.

Output > Advanced output (Ctrl+Shift+A) opens the VJ show's video output over the whole window, as on a media server
(`src/vj/AdvancedOutput.tsx`). On the left are the screens, each with its slices and masks and an eye to hide one. In
the middle, **Input selection** shows the live composition with each slice's rectangle to move or resize, and **Output
transformation** shows the selected screen as it will look: each slice cut from the composition and placed on its four
corners in perspective, with the masks over them in black (soft edges are left to the real output). On the right are the
properties of the screen (name, display), slice or mask selected. It uses the same mapping commands as the Video page
of Preferences, which a VJ show does not list, together with the pages it has no use for (Timeline, Metronome, Voice
operator, LTC, MTC and OSC destinations).

The VJ screen (`src/vj/`) shows the layers as strips with a fader each for master (M), audio (A), video (V) and
transition time (T), the deck's tabs under the grid and the crossfader. Its Output panel shows what goes to the screens before their mapping: `vj_output_frame` decodes the playing clips through the same deck as the screens, places them as the screens do (`video_output::pictures`) and draws them small on the CPU (`flux_render::composite`, bilinear, in linear light with the same blends), and the monitor puts the pixels on a canvas thirty times a second, with two asks on the way at most. The VJ screen stops asking for the picture and the play state while its window is hidden; the screens play on. `flux-render` and `jpeg-encoder` are built optimized in dev builds too, so the monitor keeps up in `npm run tauri dev`. A clip's `scale` and `position` (set in the inspector by dragging the number's handle, as in Resolume, or with the arrow keys) move it after its fit (`flux_render::transformed`), on the screens and the monitor alike. While a fader or a transform is dragged, `vj_preview` moves the value in the project the backend holds, without the project going back and forth, and only the newest move is sent while one is on its way; `vj_edit` keeps the value when the hand lets go. In a VJ project `video_thumbnails` decodes only the first frame of a video (`flux_video::poster`), and the waveform and tempo of its sound are not read, so an imported clip is ready sooner; each clip shows while it is importing, being read, or unreadable.

A VJ project also takes still images (PNG, JPEG, WebP, BMP, TIFF, GIF), imported like videos. FFmpeg reads one as a
video of one frame (`VideoFile` opens a file that has no length as one frame, and does not seek a file still at its
start), `video_probe` answers `still: true` for it, and `grid_frame` keeps showing it until it is stopped, whatever its
playback; the play state gives it no `progress` or `remaining`. The media browser filters its items by kind: all, video
or image.

Generators are clips whose picture Flux makes itself: a solid colour, a strobe, a gradient, colour bars and noise
(`flux_project::generator`). The media browser lists them under its Generators tab and they are dragged onto the grid
like media; the inspector edits their colours and numbers (`clipGenerator`, previewed live through `vj_preview` while a
number is dragged or a colour picked). `GridPlay` gives a generator's layer the milliseconds since its launch instead of
a video frame, stretched over the composition, and never ends it; `VideoDeck` makes the picture on the CPU
(`flux_video`'s `generate.rs`) only when it changes and keeps it per clip: one pixel for a solid colour or a strobe's
flash, a 16:9 picture for a gradient (so its angle is the one drawn), 1792 x 1 for the bars and 320 x 180 for the noise,
each stretched when drawn. The picture's index changes with its parameters and its phase, so a screen uploads it again
only then. Between a strobe's flashes the layer shows nothing. The screens and the output monitor draw them as any other
picture.

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
