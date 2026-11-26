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

For the operator's view of VJ mode see the [VJ mode guide](vj-mode.md), and the references for every
[effect](vj-effects.md), [generator](vj-generators.md) and [blend mode](vj-blend-modes.md). This section is for developers.

A VJ project ([project format](fxtc-format.md#vj-grid-version-14)) shows its grid instead of a timeline. Its clips play
on a clock of their own, not the transport's: `flux_video::GridPlay` holds which clip plays on each layer and when it was
launched, with a playhead for each (see [Clip transport](#clip-transport)), and `frame_at` maps the playhead to a frame of the video, looping, stopping after one pass or
going back and forth (`playback`) inside the clip's region. The output's render threads ask `VideoDeck::show_grid` for the pictures on every
refresh, so a launched clip has a player only while it plays. A layer's picture is its `master` times its `opacity`
times its crossfader side: each side shows fully from its end to the middle and fades out over the other half. `bypass`
hides a layer and a `solo` layer hides every layer that is not soloed. Clip sound is not played yet, so `volume` does
nothing for now.

A layer with a `transition` time keeps drawing the clip it replaces until the new one has faded in: with the `alpha`
effect the new clip fades in over the old one, with `add` the old one fades out while the new one fades in. Launching the
clip that already plays restarts it at once. A clip launched on a layer that plays nothing fades in, and a stopped layer (by hand, by a column without a clip on it, or a piano clip let go) fades out, over the same time. Changing the transition time while a transition runs makes it go on from where it is at the new speed (`GridPlay::retime`, on every command and read of the play state). Each layer is composited over the layers below it, as Resolume does: with its own alpha times its opacity as the cover, `result = mix(below, blend(below, picture), cover)`, so where a layer is clear (a Text Block's background, a PNG's transparent pixels, an empty layer in between) what is below shows untouched, and the opacity fades between the light below and the blended one. The blends (`flux_render::Blend`, the same formulas on the GPU and in the monitor's CPU twin `composite.rs`; the layers are mixed in sRGB-encoded (gamma) values, as Resolume does, so Multiply, Overlay, Screen and the opacity fades look the same as there) are, as the layer strip groups them: `alpha`; `darken`, `multiply`, `colorBurn`; `lighten`, `screen`, `colorDodge`; `overlay`, `softLight`, `hardLight`, `linearLight`, `vividLight`, `pinLight`; `add`, `subtract`, `difference`, `differenceInverted`, `exclusion`; `hue`, `saturation`, `color`, `luminosity`; and the keys and masks `alphaCut`, `lumaKey`, `lumaKeyInverted`, `lumaIsAlpha`, `mask50`, `rgb`, `displace`. The separable ones are the W3C Compositing and Blending formulas, linear, vivid and pin light the usual Photoshop ones, and the four non-separable ones the W3C `SetLum`/`SetSat` ones. Every blend, `alpha` included, needs the light below, so a frame is drawn into the composition's textures (the path of the composition effects, with none applied): each layer copies the texture drawn so far into the other one and reads it as it draws (one copy and one draw per layer). The textures hold linear light, which the effects and the window's sRGB output expect; the blend shader encodes the light below and the picture to sRGB, mixes them, and decodes the result. The keys and masks work on encoded values like the rest, with `luma` the W3C `0.3 R + 0.59 G + 0.11 B` of the layer's colour, and `cover` the layer's alpha times its opacity (then times the soft edge and masks, as always); Resolume's page leaves the details open, so these are the standard readings: `alphaCut` is a hard cut, the layer replacing what is below where `cover >= 0.5` and leaving it elsewhere; `lumaKey` shows the layer by `cover × luma` (dark parts transparent) and `lumaKeyInverted` by `cover × (1 - luma)`; `lumaIsAlpha` shows it by `opacity × luma` wherever the layer has a picture, its own alpha ignored; `differenceInverted` is `1 - |below - layer|`; `mask50` is not drawn, it multiplies what is below by `luma` (white keeps it, black blacks it out, grey half) by `cover`; `rgb` takes the layer over one channel at a time as `cover` rises, blue up to a third, green up to two thirds, red to the end (Resolume says only that it removes the blue, green and red channels until the layer is fully revealed); `displace` is not drawn, it shows what is below read sideways by `(luma - 0.5) × 20%` of the width (white pushes it left, black right, mid grey leaves it), at the pixel rounded and clamped to the picture's edges, by `cover`. Resolume's Displace has no amount we know of, so the 20% is ours. A layer that is not `alpha` never hides the ones below it.

The app crate's `vj_commands.rs` holds what plays, which is never stored. Its commands answer with what plays now
(`{ playing: [{ layer, clip, seconds, position, progress, remaining, region, cues, rate, passSeconds, level, transition, stopping }], column, crossfader }`:
`position` is where each clip's playhead is in the whole clip, `progress` where it is in its region, `level` how much of it reaches the output, `transition` how far its
layer's transition is while one runs, `stopping` whether the layer is fading out, and `column` the column launched last while its
clips play):

| Command | Arguments | Does |
| --- | --- | --- |
| `vj_launch` | `clip` | Plays the clip on its layer from its first frame, in place of what played there, over the layer's transition; a playing clip starts again. |
| `vj_release` | `clip` | Stops a `piano` clip when it is let go; a `normal` clip keeps playing. |
| `vj_launch_column` | `column` | Launches every clip of the column at once; a deck layer without a clip in it stops and global layers are left as they are. |
| `vj_stop` | `layer?` | Stops the layer, or every layer. |
| `vj_scrub` | `layer`, `position` | Moves the playhead of the clip playing on the layer to `position`, 0 to 1 of the clip's length (brought into its region). |
| `vj_cue` | `clip`, `index` | Jumps the clip to its cue `index` (0 to 7), launching it there if it is not playing; refused when the cue is empty. |
| `vj_crossfader` | `position` | Moves the crossfader, 0 (side A) to 1 (side B). |
| `vj_play_state` | | Reads what plays. |

A VJ show has a menu bar and keys of its own (`src/vj/menus.ts`): File, Edit, Composition, Deck, Layer, Clip, Output,
View and Help, with no tracks, timelines or transport. Its keys: 1 to 9 launch a column of the deck shown, Shift+1 to 8 jump the selected clip to its cues, Enter the
selected clip, Ctrl+PgUp/PgDn change deck, Ctrl+. stops everything, B, S and X bypass, solo and stop the selected layer,
Del removes the selected clip, Ctrl+I imports media, Ctrl+Shift+A opens the output and Space plays or pauses the
timecode on the internal clock. The timeline's other keys (Home, the arrows, N, S, G, F2, Ctrl+1/2) do nothing in a VJ
show.

The Composition bar under the grid holds the show's tempo and its timecode clock, as in Resolume, and the crossfader
sits under the layer headers beside the deck tabs. The tempo is the BPM of the show's one timeline, so the metronome and
the engine follow it: typed, nudged a BPM, halved, doubled or tapped (`setBpm`, `nudgeBpm`, `scaleBpm`, `tapBpm`; the
backend reads the tempo from the last run of taps, `flux_project::vj::tapped_bpm`). The clock's source is `vj.clock`:
on `internal` it is Flux's transport, played, paused, stopped back to the start and typed while it does not run (a seek);
on `ltc` or `mtc` it shows the timecode another machine sends, its keys are gone and it cannot be typed. Where that
timecode comes in is the machine's, chosen in Preferences > Remote control > Timecode input (listed only for a VJ show):
an audio input and channel for LTC, a MIDI input for MTC (`ltcIn`, `mtcIn` in the preferences). The backend opens the
input when the clock first asks (`timecode_in_status`) and closes it when the clock goes back to `internal` or the
screen closes (`timecode_in_close`). `flux_engine::chase` decodes it: LTC from the edges of its biphase signal, its rate
from the drop-frame flag and the length of a word; MTC from eight quarter frames (two frames on) or a full-frame SysEx.
The clock shows the incoming time and rate, carried on between readings while it runs, and the tag beside it lights while
the timecode runs; a click on the tag opens the page. Flux's own transport does not chase it. A layer folds to a thin row with the arrow beside its number, its right-click menu or Layer > Collapse layer, as in
Resolume; it keeps playing, and the fold is saved with the show (`collapsed`).

Output > Advanced output (Ctrl+Shift+A) opens the VJ show's video output over the whole window, as on a media server
(`src/vj/AdvancedOutput.tsx`). On the left are the screens, each with its slices and masks and an eye to hide one. In
the middle, **Input selection** shows the live composition with each slice's rectangle to move or resize, and **Output
transformation** shows the selected screen as it will look: each slice cut from the composition and placed on its four
corners in perspective, with the masks over them in black (soft edges are left to the real output). On the right are the
properties of the screen (name, display), slice or mask selected. It uses the same mapping commands as the Video page
of Preferences, which a VJ show does not list, together with the pages it has no use for (Timeline, Metronome, Voice
operator, LTC, MTC and OSC destinations).

The VJ screen (`src/vj/`) shows the layers as strips with a fader each for master (M), audio (A), video (V) and
transition time (T), the deck's tabs under the grid and the crossfader. Its Output panel shows what goes to the screens before their mapping: `vj_output_frame` decodes the playing clips through the same deck as the screens, places them as the screens do (`video_output::pictures`) and draws them small on the CPU (`flux_render::composite`, bilinear, the layers mixed in encoded values with the same blends), and the monitor puts the pixels on a canvas thirty times a second, with two asks on the way at most. The VJ screen stops asking for the picture and the play state while its window is hidden; the screens play on. `flux-render` and `jpeg-encoder` are built optimized in dev builds too, so the monitor keeps up in `npm run tauri dev`. A clip's `scale` and `position` (set in the inspector by dragging the number's handle, as in Resolume, or with the arrow keys) move it after its fit (`flux_render::transformed`), on the screens and the monitor alike. While a fader or a transform is dragged, `vj_preview` moves the value in the project the backend holds, without the project going back and forth, and only the newest move is sent while one is on its way; `vj_edit` keeps the value when the hand lets go. In a VJ project `video_thumbnails` decodes only the first frame of a video (`flux_video::poster`), and the waveform and tempo of its sound are not read, so an imported clip is ready sooner; each clip shows while it is importing, being read, or unreadable.

A clip launched on the grid plays the sound of its video, at the level of the layer's A fader (`Layer.volume`) times its master and its crossfader side; the opacity is the picture's and does not touch it. A bypassed layer, or any layer while another is soloed, is silent. The backend builds the sound (`vj_audio::refresh`, from the grid, the project and the sound decoded for the cues, `video:<id>` in the audio cache) and leaves it to the output's audio callback through a mutex the callback only `try_lock`s (`flux_engine::grid_audio`), so it never waits; the callback mixes it on the programme's channels whether the transport plays or not. Sound and picture wrap together (Loop, Once, PingPong, which plays the sound backwards on the way back), and a layer's transition fades the sound in and out as it does the picture. A video is decoded on a thread of its own the first time a clip of it plays, and its sound joins in step when ready. The sound needs the output to be open, as the VJ screen does; stills and generators have none. The routing of tracks to interface channels (Audio routing) does not carry it yet.

A VJ project also takes still images (PNG, JPEG, WebP, BMP, TIFF, GIF), imported like videos. FFmpeg reads one as a
video of one frame (`VideoFile` opens a file that has no length as one frame, and does not seek a file still at its
start), `video_probe` answers `still: true` for it, and `grid_frame` keeps showing it until it is stopped, whatever its
playback; the play state gives it no `progress` or `remaining`. The media browser filters its items by kind: all, video
or image.

Effects work as in Resolume: an ordered stack on each clip and on each layer, listed in the inspector's Clip and Layer
tabs under **Effects** (up to 16 per stack). **Add effect** picks one from a list grouped by category, and the media
browser's **Effects** tab lists them all with a line each: drag one onto a grid clip, a layer header or an inspector
stack, or double click it to add it to the selected clip (or layer). In the stack each effect folds shut (kept on the
screen only, never in the file), can be bypassed, duplicated, reset to its starting settings, moved with its grip or
with the arrows, and removed; a badge on the grid clip and the layer header counts the effects. Settings are scrubbed like
the clip's scale (dragging one moves the output live: `effectParams` through `vj_preview`), picked or switched, and
animated effects choose between a rate in Hz and a cycle in beats of the show's tempo. A clip's stack applies first,
from the top down, then its layer's, so a layer's effects also reach both clips while a transition runs.

The 36 effects of a clip or a layer, by category (settings and ranges are in [the format](fxtc-format.md#vj-effects) and, for operators, in [VJ effects](vj-effects.md)):

- **Colour:** brightness, contrast, saturation, hue, tint, invert, levels, colorize (gradient map), solarize.
- **Distort:** mirror, zoom, shift, rotate, kaleidoscope, polar, wave warp, twirl, bulge (fisheye), tile, pixelate,
  displace.
- **Blur & edges:** blur, radial blur, zoom blur.
- **Glitch:** shift glitch, twitch, RGB split.
- **Stylise:** posterize, threshold, edge detect, vignette, film grain, scanlines.
- **Time:** strobe, slide, roll (vertical hold).

The catalogue of kinds, settings, ranges and defaults is `flux_project::vj_effects::catalog()`; the interface gets it from
`vj_effect_catalog` and holds no range of its own. The ops are `addEffect`, `removeEffect`, `moveEffect`, `effectParams`,
`effectBypass`, `duplicateEffect` and `resetEffect`.

How they are drawn: the backend turns a picture's stack into one program, `flux_render::Effects`, each time it draws
(`Effects::new(stack, Time { seconds, beats })`: the seconds since the clip was launched and the beats the grid's clock
has counted at the show's tempo, `Grid::beats`), and the shader interprets it in the same single pass as the rest, on the
screens and, step for step the same, in the monitor (`flux_render::composite`). A step is a kind and twelve numbers (at
most 32 steps a picture, 16 pictures: 33 KB of uniform), in three stages that keep the order of the stack within each:

1. **Where:** the distortions walk back from the last to the first, mapping each output pixel to where the picture is read
   (a zoom out or a rotation leaves transparent edges, a shift wraps, the others bounce at the edges). Mirror, shift,
   kaleidoscope and the rest all live here.
2. **How:** the picture is read there. Blur, radial blur and zoom blur share one pass of 32 taps and their offsets add (two
   blurs add in quadrature, up to a full blur); RGB split, and the colour offsets of shift glitch, pull the red and the
   blue to either side; edge detect looks at the result with a Sobel filter and shows only the edges, or adds them.
3. **What colour:** the colour steps apply first to last. Consecutive affine ones (brightness, contrast, saturation, hue,
   tint, invert, a lit strobe, a twitch's flash of light) fold into one 3x4 matrix on linear light; posterize, threshold,
   colorize, solarize, levels, vignette, noise and scanlines run in their place, on a gamma 2.2 scale where it matters.

The stages do not interleave, so a blur below a threshold in the stack still blurs first; this is the price of one pass.

Time is worked out on the CPU for each frame, so the shader only sees numbers: a spin is an angle, a scroll an offset, a
strobe or a twitch whether it is on. A paced effect counts cycles as seconds times its rate, or beats over its beat length,
and the whole cycle number (kept exact for days) and the fraction of it go to the shader. Randomness (shift glitch bands,
twitch jumps, grain, displacement noise, roll jitter) is a hash of the effect's id, that cycle number and the pixel or the
row, never a generator: the same frame is the same everywhere. Because the tempo moves the beat count, the count keeps its
phase when the tempo changes. Aspect-correct effects (rotate, kaleidoscope, twirl, bulge, radial blur) use the picture's own
shape, so a clip stretched to a different shape distorts them a little.

The monitor draws the same program on the CPU, with two cheaper approximations: its blur pass takes 16 taps instead of 32
(`flux_render::effects::CPU_TAPS`), and it samples bilinearly at 480 pixels wide rather than at the screens' resolution,
so fine noise and one pixel edges differ from the screens. A picture with no effects is drawn exactly as before; one with
effects never hides what is below it.

### Composition effects

The composition has a stack of its own, as in Resolume: it applies to the final mix of all the layers, before the output
mapping, and it is the **Composition** tab of the inspector (next to Clip and Layer; up to 16 effects, with the same
add, fold, bypass, duplicate, reset, reorder, remove and modulation controls). All the effects above work there, as one
program on the whole composition, and there are three more, in the **Feedback** category, that only exist there and are
tagged *Composition* in the browser (a clip or a layer refuses them): **Feedback**, **Trails** and **Bloom**.

- **Feedback** draws the previous output again, zoomed (`zoom`), turned (`rotate`), moved (`x`, `y`) and with its hue
  turned (`hue`), and puts it with the new frame (`blend`: `mix` goes `amount` of the way to the old frame, `add` adds it
  times `amount`, `lighten` keeps the lighter). The old frame is drawn again every frame, so what it carries shrinks into
  or grows out of the middle: tunnels and spirals. With an `amount` of 0 it is no effect.
- **Trails** keeps `decay` of the previous output with the new frame (`mode`: `blend` goes `decay` of the way from the new
  frame to the old one, an average that never gets brighter, `lighten` keeps the lighter of the two, `add` adds the old
  frame times `decay`): what moves leaves a trail that fades. With a `decay` of 0 it is no effect.
- **Bloom** takes what is brighter than `threshold`, spreads it over `radius` (a chain of four blurs, each half the size
  of the one before) and adds it back `intensity` times.

Drop an effect on the Output panel or on the composition bar to add it to the composition, or double click it in the
browser while the Composition tab is shown (a composition-only effect always goes there and opens the tab). When the
stack has effects, the Output panel and the composition bar show an **FX n** badge and a switch that bypasses the whole
stack at once (`compositionBypass`), which is for the stage: the effects keep their place and settings.

**Render order.** Effects keep the order of the stack within three groups, and the groups run in this order: the history
effects (feedback and trails), then the program of every other effect, then the blooms. So a bloom is always the last
thing to touch the picture, and a blur below a feedback in the stack still runs after it. At most two blooms count.
Each frame the screen draws:

1. every picture of the layers, with its own effects, blend and opacity, into a texture of the composition's size (half
   floats when the graphics adapter can draw on, filter and blend them, else 8-bit sRGB);
2. each history effect, reading that texture and the history;
3. the program of the other effects, as one picture the size of the composition;
4. a copy of the result into the history, for the next frame;
5. each bloom: a bright pass at half the size, three more blurs each half the size, added back up the chain and onto the
   picture;
6. the texture drawn on the window through the slices and masks, as one picture the size of the composition, so the
   advanced output mapping works as always.

With no effect in the stack (or all bypassed) none of this happens: the pictures go straight to the window as before.

**History.** What the history holds is what steps 2 and 3 made, so a feedback draws the picture the viewer saw, less its
bloom (a glow that is drawn again every frame piles its light up until the frame is white). Each output window keeps its own history, in its own GPU memory, and the editor's monitor keeps one on the
CPU, so a feedback on two screens is two independent tunnels that start from black when the window opens. The history is
forgotten when the composition changes size and when the stack is empty or bypassed for a frame, so bringing it back
starts clean. The test card the slices are lined up with is never changed by the effects. Time is a frame step:
`amount`, `decay` and `zoom` are counted per 60th of a second and the movements per second, all scaled by the time since
the last frame (at most a quarter of a second, so a stall does not throw the picture), which makes a tunnel the same at
30 and at 120 frames a second. The program's own time is the grid's clock, like a layer's effects, and the composition's
settings are modulated like any effect's.

The monitor draws the same on the CPU (`flux_render::composite_post`, with a `History` it keeps between frames) with one
approximation: its bloom averages 16 pixels for each level where the screens take four filtered taps, so a glow can
differ by a little.

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

The **Text Block** generator (Resolume's) draws lines of text over a clear background, so what is below shows through. The
`flux-text` crate does it in pure Rust: `fontdb` finds the system's fonts, `rustybuzz` (a HarfBuzz port) shapes each line
(GPOS and `kern` kerning, ligatures, marks; ligatures are off while `tracking` is not 0) and `ab_glyph` draws the glyph ids it
returns, from the same font bytes. A family or face the machine lacks falls back to the bundled Inter (a Latin subset of
Regular and Bold, SIL OFL; `crates/flux-text/fonts`), so a text always draws. A character the chosen face lacks (Cyrillic,
CJK, symbols, emoji) is drawn with another face: a line is cut into runs by font coverage, and each run is shaped with the
chosen face first, then the first face of the system whose cmap has the character (the same slant and the nearest weight
first, one with an outline before one that only has bitmaps; the choice per character is cached), then the bundled Inter; a
character nobody has draws the chosen face's own missing-glyph box. Zero width joiners and variation selectors stay with the
character before them. A colour emoji that a font only has as bitmaps (Noto Color Emoji, Apple Color Emoji) has no outline
to draw, so it leaves a blank of its advance; a font that has outlines for it (a monochrome emoji or symbol face, or the base
glyph of a COLR font) draws it in the block's colour, never in its own colours. `vj_fonts` lists the families and their faces for the inspector's
pickers. The picture is the shape of the composition, at most 1920 wide, straight RGBA with the glyphs' alpha, and is made
once per set of parameters: its index hashes the generator, the line shown and the composition's size, so a still text is
rasterised once and a change (a typed letter, a scrubbed size) makes one new picture. With `lines` `one`, the line shown is
`line` plus the whole beats of the grid's clock divided by `advance` (1, 2, 4, 8 or 16; 0 is by hand), going round the lines
that are not empty. Stepping on the grid's beats rather than the clip's launch keeps the changes on the beat. The previous
and next buttons are the backend op `textLine` (`step` -1 or 1): `line` is stored in the clip, so it enters the undo history
and a saved show opens where it was left. The play state (`vj_play_state`) carries `line` for a playing text block that shows
one line at a time: the line on screen now, which the inspector's readout shows as `shown / total` (the stored `line`
while the clip does not play).

The numbers of a generator are modulated like a clip's `scale` (see [Modulation](#modulation)): a text block's `size`,
`opacity`, `positionX`, `positionY`, `rotation`, `tracking` and `leading`, a strobe's `rate` and `width`, a gradient's
`angle` and a noise's `rate`, each with the "~" button next to its scrub, as the field `generator.<name>` of the clip, with
the range of the generator's own limits. Drawing the text again is the cost of a moving number, so three things keep it
cheap. A modulated value is rounded to a step of about a pixel (`generator::NUMBERS`: 0.001 of the height for the size, 0.1
degree, 1/255 of opacity...) when it is put in the drawn copy of the grid, so the picture's index, which hashes the
generator, changes only when the value moves a step: a held or slow value costs nothing. The deck keeps the last four
pictures of each generator clip, so the screens that ask a moment apart, and the output monitor, find the one another made
instead of drawing their own. And `flux-text` keeps the coverage of the last eight blocks (the glyphs' alpha, before the
block is placed): a modulated position, rotation or opacity only resamples it, and the glyphs are drawn again only when the
text, face, size, tracking or leading change. A modulated `size`, `tracking` or `leading` therefore draws the glyphs at
most once per step of the value.

### Clip transport

Each clip of the grid has a transport, as in Resolume (fields in the [project format](fxtc-format.md#vj-grid-version-14)):
a `speed` (0.05 to 8), a `direction` (forward, reverse or pause), a `tempo` (free, or one pass of 1 to 128 beats), a
`region` (the part of the clip that plays, with in and out points) and 8 `cues`; a still image or a generator also has a
`duration`. The Clip tab of the inspector has a
Transport section: a strip of the clip's thumbnails with the region shaded and its two handles (dragged live through
`vj_preview`, kept on release), the playhead, the cue marks, and a click or drag on it scrubs (`vj_scrub`); the
direction, a speed scrub with a 1x reset, the tempo (Free or 1 to 128 beats, with the length of a pass the backend
reports) and 8 cue buttons: a click jumps (`vj_cue`), an empty one is set at the playhead, Alt-click or the x clears
it. A clip that does not play forwards at its own speed shows a badge on its cell (a reversed or paused clip, the speed
or the beats). The UI never computes a position: it draws `position`, `region` and `cues` from the play state.

**The playhead.** `flux_project::playhead` holds the math, which the picture, the sound and the play state share. A
launched clip has an anchored `Playhead`: where it was (`u`, counted in lengths of the region from its start, so 1 is the
end of the region and it goes on past it when the clip loops) when the grid's clock read some time and had counted some
beats. At a later tick `u` is the anchor plus the time since times the rate (the speed times the direction, over the
length of the region in seconds) while the tempo is free, or the beats since times the direction over the beats of a
pass when it is a number of beats, which follows tempo changes and TAP exactly (the grid's beat count keeps its phase
across a tempo change). `fold` then turns `u` into a place in the region by the clip's `playback`: `loop` wraps,
`once` ends (forwards past the end of the region, backwards past its start, and the layer then shows nothing) and
`pingPong` bounces. A clip launched in reverse starts at the end of its region. Any change of the clip's speed,
direction, tempo, region or playback anchors the playhead again from where it is (`GridPlay::follow`, called with every
tick of the grid's clock by the screens, the sound and the commands), so nothing jumps; a region that moves under the
playhead keeps its place (brought into the new region) and, in a ping-pong, the leg of the bounce. A `scrub` or a `cue`
anchors it at the place asked, brought into the region, and wakes a clip that played once and ended. A clip a
transition leaves keeps its own playhead. A still image and a generator have no file to
run over: their length is the clip's `duration` (10 s by default, 0.1 s to 1 h, the op `clipDuration`), and the playhead
treats them as a video of that length. The strip moves, `once` ends after the duration (the layer shows nothing), `loop`
restarts, a beat tempo still sets the pass, and the region and cues work. A generator's time (its strobe and noise phase) is
the place in that length, in milliseconds, so a loop starts it over. The Duration scrub is shown in the Transport section
of a still or a generator clip only, and the cell's countdown counts down what is left of it. Effects animate on the real
time since the launch, whatever the speed. The speed is not modulatable: integrating a modulated rate over time is not
done.

**The sound.** A grid voice (`flux_engine::grid_audio`) follows the same playhead as a `Course`: where `u` was at an
instant of the grid's clock and how fast it moves, folded by the same function, so sound and picture agree. The audio
callback reads the decoded sound at a fractional frame with linear interpolation (varispeed: the pitch follows the speed
and the tempo), backwards in reverse, and a paused clip is silent. The callback cannot count beats, so a clip that lasts
a number of beats is turned into its rate when the mix is built; the mix is built again when the tempo changes
(`modulation.rs`' timer checks it for clips with a beat tempo) and whenever what plays changes. When a new mix takes a
voice somewhere else (a scrub, a cue, a clip that ends or starts moving), the old place fades out and the new one in over
one mix chunk (256 frames, about 5 ms) so the jump does not click; the wrap of a loop is not smoothed.

### Modulation

A numeric setting of a clip, a layer or an effect can be driven by itself, as in Resolume: each has a "~" button in the
inspector that opens a sub-row to choose the source and the minimum and maximum it moves between. The stored form is in
the [project format](fxtc-format.md#vj-modulation). Everything is worked out by the backend; the interface only draws the
sub-row, and marks on the control where the backend says the value is now (`vj_modulation_values`, asked for thirty times a
second only while the inspector shows something modulated).

**LFO.** The shapes are the sine, triangle, saw up, saw down, square, random steps and smooth random. A cycle goes from 0 to
1: the sine and the triangle start at 0 and peak half way, the saw up rises and drops, the saw down falls and jumps back,
the square is 1 for the first half. The random shapes draw a level for each cycle from a hash of the modulation's key and
the cycle's number, never from a generator, so a stop and a start repeat them; the smooth one reaches the next cycle's
level along a half cosine. The pace is `free` (cycles a second, 0 to 30) or `beats` (one cycle every 1/4 to 16 beats of the
tempo), and the phase shifts where the cycle starts.

**Timebase.** An LFO runs on the composition's clock, the grid's own: seconds since it started for a free pace and the beats
`Grid::beats` has counted for a synced one (which keeps its phase when the tempo changes), never on a clip's time since
its launch. Every LFO of the grid stays in phase whatever clips are launched when, and two of them with the same pace and
phase move together.

**Audio.** A source follows the low (below 250 Hz), mid (250 Hz to 4 kHz) or high (above 4 kHz) band of the sound, or its
level, 0 to 1, times a gain and cut at 1. Each band is scaled by a slow peak follower of its own (ten seconds), so typical
music sits near 0 to 1 whatever its loudness, and a sound below a floor reads 0. `attack` and `release` smooth what the
modulation follows, in seconds, and are applied where it is evaluated (`Modulator`), so every screen that asks sees the same
answer.

**Analysis.** `flux_engine::analysis` reads mono samples in windows of 2048 with a Hann window and a real FFT every 512
samples, and leaves the four readings in atomics that the video frame step reads without a lock. What it listens to is
chosen in Preferences, on the VJ-only page *Audio analysis*, and belongs to the machine: the grid's own sound (the default),
one channel of an audio input (opened with cpal as the LTC input is, and tried again every two seconds if it fails) or
nothing. For the grid's sound the output's audio callback copies the grid's own mix, apart from what it is added to, into a
lock-free single producer ring buffer of 65536 samples (no allocation, no lock, and a full ring drops the chunk); a thread
of the app drains it and analyses it, and feeds silence when nothing arrives so the bands fall away. The composition bar
has a small meter of the four readings (`vj_analysis_levels`) that opens the page.

**Drawing.** Each frame step of the screens and of the monitor asks `modulation.rs` for the grid with its modulated values
in place of the stored ones (`flux_project::vj_modulation::modulate`: effect settings, layer `master` and `opacity`, clip
`scale` and `position`, and a generator's numbers), and draws that. The stored project is never changed, a whole-number setting is rounded and every
value is cut to its range. A grid with no modulation is not copied.

**Sound.** A layer's `volume` and `master` reach the grid's sound, which is built ahead of the audio callback as a
`GridMix`. While any of them is modulated, a timer rebuilds the mix from the modulated grid thirty times a second (`vj_audio::refresh`),
and the callback smooths each voice's gain from where it was to the new one across a chunk. Nothing is evaluated in the
callback, which would have to share the clock and the analysis with it; the price is a gain that moves in 33 ms steps.

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
