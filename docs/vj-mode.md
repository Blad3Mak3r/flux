# VJ mode

VJ mode turns FLUX into a clip launcher for video, in the manner of a media server: a grid of clips you launch by hand,
layers mixed into one composition, and that composition sent to your screens. This page is the overview. The details
are in three reference pages:

- [VJ effects](vj-effects.md): all 39 effects, their settings and the effect stacks.
- [VJ generators](vj-generators.md): the six clips FLUX draws itself, with the Text Block in depth.
- [VJ blend modes](vj-blend-modes.md): all 29 ways a layer mixes with what is below.

For mapping the composition onto projectors and screens see [Video and projection mapping](video-output.md). For driving the show from a MIDI controller or OSC see [Map mode](#map-mode-midi-and-osc) and [Remote control](remote-control.md#vj-shows).

## A VJ show is a kind of project

A project is either a **timeline** project (tracks, cues and timecode) or a **VJ show**. The kind is chosen when the
project is created (**New VJ show** or **New cue show** in the File menu) and **never changes**. A VJ show has its own
menus (File, Edit, Composition, Deck, Layer, Clip, Output, View, Help) and none of the timeline's tracks or
transport.

## The grid

| Part | What it is |
| --- | --- |
| **Layers** | Rows, drawn from the bottom of the grid to the top, each one mixed over the layers below it. A grid has 1 to 16 layers; a new show starts with 3. |
| **Columns** | The columns of a deck, up to 64. A new show starts with 8. Launching a column starts every clip in it at once. |
| **Decks** | Sets of columns, up to 32, switched by tabs under the grid while the layers stay. Ctrl+PgUp and Ctrl+PgDn change deck. |
| **Global layers** | A layer added as global keeps its clips in slots that are the same in every deck, so they stay when the deck changes. |
| **Clips** | A video, a still image or a [generator](vj-generators.md), at most one per cell. |

Import media with **Ctrl+I** (videos and still images: PNG, JPEG, WebP, BMP, TIFF, GIF) or drag files in, then drag
them from the browser onto cells. Generators are in the **Generators** tab of the browser, and effects in the
**Effects** tab.

## Launching clips

Click a clip to launch it on its layer, from its first frame, in place of what was playing there. Launching a clip that
already plays starts it again. Click an empty cell to stop the layer. A clip's **Trigger** (in the Clip inspector)
chooses what a press does:

| Trigger | A press |
| --- | --- |
| Normal | Launches the clip, which plays on until something else replaces it. |
| Piano | Plays the clip while the button is held and stops it when released. |

Other ways to launch:

- Keys **1** to **9** launch a column of the deck shown. **Enter** launches the selected clip. **Ctrl+.** stops everything.
- A folded layer (see below) has no room for its cells, so a small **square** in the row launches its clip.
- A cell's select control, or the Preview panel, lets you look at a clip without launching it.

## The layer strip

Each layer has a header with:

| Control | Does |
| --- | --- |
| **X** | Stops the layer: its clip is ejected. |
| **B** | **Bypass**: the layer shows and sounds nothing, though its clip keeps playing. |
| **S** | **Solo**: while any layer is soloed, only the soloed layers show. |
| Blend menu | The layer's [blend mode](vj-blend-modes.md). |
| **M** | **Master**: scales both the picture and the sound of the layer. |
| **A** | **Audio**: the level of the sound of the layer's clips. |
| **V** | **Video**: the opacity of the layer over the layers below. |
| **T** | **Transition** time, 0 to 10 s. 0 is a cut. |
| Transition effect | **Alpha** (the new clip fades in over the old one) or **Add** (the old one fades out while the new one fades in). |
| Side A / B | Which side of the crossfader the layer is on. |
| Fold arrow | Folds the layer to a thin row. It keeps playing; the fold is saved with the show. |

With a transition time above 0, a clip launched on an empty layer fades in, and a stopped layer fades out over the same
time. A layer's effects apply to both clips while a transition runs. The selected layer's shortcuts are **B**, **S** and
**X** too. Right-click a layer for the rest (move up and down, rename, Autopilot, remove).

### Autopilot

A layer's **Autopilot** (the Layer tab of the inspector, or the layer's right-click menu) makes it play its clips one
after the other by itself, as in Resolume. It is **Off**, **Next**, **Prev** or **Random**. When the clip on the layer
ends a pass, the layer launches the next or previous clip of the same layer in the deck shown, or a random one, through
a normal launch: the layer's transition and the T fader apply. Empty cells are skipped and the order wraps around;
Random never repeats the clip that just played when the layer has another.

A pass is one trip through the clip's region: a video's end, or a still image's or a generator's **Duration**. With
**Loop** the layer moves on at the end of each pass, with **Once** when the clip ends, with ping-pong after the full trip
there and back, and with a beat **Tempo** at the end of its beats. A paused clip never ends. A layer with an autopilot
shows a small **AP** badge in its strip. Launching a clip by hand still works; stopping the layer (**X** or the eject
button) stops the autopilot until a clip is launched again. Global layers follow their own clips. The setting is saved
with the show.

## The crossfader

The crossfader sits under the layer headers beside the deck tabs. Put layers on side **A** or side **B**; a layer on
neither is not touched. A side shows fully from its end of the fader to the middle and fades out over the other half.
Double click the fader to centre it. The Composition menu can set it all A, middle or all B.

## Output and Preview

- **Output** shows the composition: what the screens show before their mapping, with each playing clip drawn at its
  place. It is a small monitor; the screens show the video itself.
- **Preview** shows the selected clip **alone, as it lands in the composition**, with its fit, scale, position and
  effects, playing on its own whatever is on air. Select a clip to see it without launching it, then press
  **Launch on** the layer to put it on air.

Both stop updating while the window is hidden; the screens keep playing.

## The BPM bar

The composition bar under the grid holds the show's tempo, which the clips' beat-based pacing, the effects set to Beats
and the modulations all follow.

| Control | Does |
| --- | --- |
| **BPM** | The tempo, 20 to 400. Type it, or use the minus and plus buttons for one BPM at a time. |
| **Beat lights** | The beat in its bar of four, with a square that flashes on each beat. They go dim while the beats are held. |
| **TAP** | Tap on the beat: the tempo follows your last taps (up to eight, with gaps shorter than two seconds). |
| **÷2 / ×2** | Halves or doubles the tempo. |
| **Nudge** (the two arrows) | Hold to run the beats a little slower or faster, to line up with the music. Let go to go back to the tempo. |
| **RESYNC** | Makes this moment the first beat of a bar. |
| **PAUSE** | Holds the beats; press again to go on from the same phase. |

The beat count keeps its phase when the tempo changes, so everything that follows the beats stays in step.

## The timecode clock

Next to the tempo is the show's **timecode clock**. Its source is chosen with its tag:

| Source | Behaviour |
| --- | --- |
| **INT** | FLUX's own clock. Play, pause, stop (back to the start) with the buttons or **Space**, and type a timecode and press Enter to go there. |
| **LTC** | Shows the LTC timecode another machine sends. It cannot be typed or moved here. |
| **MTC** | Shows the MTC timecode another machine sends. It cannot be typed or moved here. |

Where LTC and MTC come in is chosen in Preferences > Remote control > Timecode input. Click the tag to open that page. FLUX
shows the incoming timecode; it does not chase it with its own transport.

## Clip transport

Each clip has a transport in the Clip inspector, as in Resolume:

| Control | Range | Does |
| --- | --- | --- |
| **At the end** | Loop, Once, Ping-pong | What the clip does when it reaches its end (or the end of its region). |
| **Direction** | Forward, Reverse, Pause | Which way it plays. |
| **Speed** | 0.05 to 8, 1 is normal | How fast it plays while the tempo is Free. Reset with the 1x button. |
| **Tempo** | Free, or 1, 2, 4, 8, 16, 32, 64, 128 beats | With a number of beats, one pass lasts exactly that many beats of the show's tempo, whatever the speed. |
| **Region** | in and out points | The part of the clip that plays and repeats. Drag the two handles on the strip. |
| **Cues** | 8 | Marks on the clip. Click one to jump to it (launching the clip there if it is not playing). Click an empty one to set it at the playhead; Alt-click or the x clears it. **Shift+1** to **Shift+8** jump the selected clip to its cues. |
| **Scrub** | | Click or drag on the strip to move the playhead. |
| **Duration** | 0.1 s to 1 h, 10 s by default | For a still image or a generator only: the length the transport runs over. |

A clip that does not play forwards at its own speed shows a badge on its cell. A video's sound follows the same
playhead as its picture, backwards in reverse and silent while paused.

Clips also have **Fit** (Fit, Fill, Stretch, Original), **Scale** (0 to 10, 1 is normal) and **Position**: drag the number
sideways to change it, Shift for fine steps, double click to reset.

## Modulation

The "~" button next to a number opens its **modulation**: the value moves by itself between a **Min** and a **Max**
you choose. Min can be above Max, which inverts the movement. The stored value is never changed: the modulation
moves it while the show plays.

What can be modulated: a clip's Scale and Position, a layer's Master, Opacity and Audio level, any number of an
[effect](vj-effects.md#settings) (not the Rate and Cycle of its pace) and the numbers of a
[generator](vj-generators.md#modulating-generator-numbers).

| Source | Setting | Range | Does |
| --- | --- | --- | --- |
| **LFO** | Shape | Sine, Triangle, Saw up, Saw down, Square, Random steps, Smooth random | The wave over one cycle. Random steps picks a new level each cycle; Smooth random glides to it. |
| | Pace | Free (0 to 30 Hz) or Beats (1/4 to 16 beats a cycle) | Speed. Beats follows the show's tempo. |
| | Phase | 0 to 1 | Where in its cycle the wave starts. |
| **Audio** | Band | Low (below 250 Hz), Mid (250 Hz to 4 kHz), High (above 4 kHz), Level | The part of the sound it follows. |
| | Gain | 0 to 8 | The band's level times this, cut at 1. |
| | Attack, Release | 0 to 5 s | How slowly the value follows the sound up and down. |

An LFO runs on the composition's clock, never on a clip's own time, so every LFO of the show stays in phase whatever is
launched when. A clip's speed is not modulatable.

**Analysis source.** An audio modulation listens to what is chosen in Preferences on the **Audio analysis** page (VJ
shows only): the show's own sound (the default), one channel of an audio input, or nothing. A small meter of the four
readings (L, M, H, Lv) in the composition bar shows what it hears; click it to open that page.

## Map mode: MIDI and OSC

**Composition > View > Map MIDI / OSC** or **Ctrl+M** turns on the map mode, as Resolume's "Edit MIDI / OSC". Every
control that can be mapped gets an outline: **dashed** when nothing drives it, **solid phosphor** with the name of its
source (`CC 7 · ch 1`, `Note C2 · ch 1`, `Pitch bend · ch 1` or the OSC address) when something does. The grid shows a
**MAP MODE** strip with the state of the MIDI and OSC inputs.

1. Click a control. Its outline turns thick amber and reads **LEARN**.
2. Move a MIDI control, press a pad or send an OSC message. The first one heard becomes the source, and the outline
   turns solid. Learn again to replace it: the latest wins, and a source drives one target at a time.
3. **Right-click** an outline to clear its mapping. **Esc** cancels a learn, and then leaves the mode (so does **Ctrl+M**
   or the Exit button). While the mode is on, a click on a control chooses it and does not use it.

What can be mapped:

| Kind | Targets |
| --- | --- |
| Launch | A clip, a column. |
| Layer | Master, Audio, Video (opacity) and Transition faders; bypass, solo, stop; the autopilot (each press steps off, next, previous, random). |
| Composition | The crossfader, a deck's tab, next and previous deck (the arrows beside the tabs), stop all. |
| Tempo | The BPM, tap, resync, pause, +1, -1, ×2, ÷2 and the two nudge keys. |
| Numbers | Every number of an effect (clip, layer or composition), a clip's speed, every number of a generator. |
| Switches | An effect's bypass. |
| Dashboard | Each of the eight knobs. |

A fader, a knob or pitch bend sets a level from **Min** (source at 0) to **Max** (source at its top). A note, or a control
change above 0, presses a button: it runs an action, sets a switch, or, on a level, jumps it to the value the velocity
gives (and to Min on release with Hold). A fader mapped to an action or a switch presses it when it leaves 0 and
releases it on return. Pitch bend presses past its centre.

### The Mappings tab

The **Mappings** tab of the inspector lists every mapping: its source, what it drives, and its options.

| Option | For | Does |
| --- | --- | --- |
| **Min**, **Max** | Levels | Where the target goes with the source at its lowest and highest. Min can be above Max. |
| **Invert** | Levels | Turns the source round, on top of the range. |
| **Trigger** | Buttons | A press runs the action or turns the switch on. |
| **Toggle** | Switches | A press flips the switch. |
| **Hold** | Buttons | On while the button is held, off when released. On a clip, Hold plays it while the pad is down, as the Piano trigger does. |

The trash button deletes a mapping. The mappings are part of the show and go through undo like any other edit. One
sweep of a fader is **one** undo step, however many values it sent. A mapping whose target is removed (a deleted clip,
layer or effect) is removed with it; a file that holds a mapping to something that is not there is refused.

### Dashboard

The **Composition** tab of the inspector holds a **dashboard** of eight knobs, as in Resolume. Drag a knob up or down
(Shift for fine steps, double click for 0, arrow keys for steps), and everything linked to it moves.

- **Link numbers** turns on the map mode and waits for you to click numbers (a fader, an effect's setting, a
  generator's number, a clip's speed, the BPM...). Each link has its own **0** and **100** values: the number goes from
  one to the other as the knob turns, and the first can be above the second.
- A knob has a name (the field above its links) and can be mapped to a controller like any other control, so one fader
  can move many things at once.
- The dashboard is saved with the show. A knob is not modulated by an LFO.

## Composition effects

The composition has an effect stack of its own, in the **Composition** tab of the inspector. It applies to the final mix
of all the layers, before the output mapping. Every effect works there, and Feedback, Trails and Bloom work only there.
Drop an effect on the Output panel or the composition bar to add it. An **FX** badge and a switch in the composition
bar bypass the whole stack at once. See [VJ effects](vj-effects.md#effect-stacks).

## Advanced Output

**Output > Advanced output** (Ctrl+Shift+A) opens the video output over the whole window, as on a media server: the
screens with their slices and masks on the left, the composition with each slice's rectangle in the middle, and the
properties of the selected item on the right. It is the same mapping described in
[Video and projection mapping](video-output.md).

## Keys

| Key | Does |
| --- | --- |
| 1 to 9 | Launch a column of the deck shown. |
| Shift+1 to 8 | Jump the selected clip to its cues. |
| Enter | Launch the selected clip. |
| Ctrl+PgUp / Ctrl+PgDn | Previous or next deck. |
| Ctrl+. | Stop everything. |
| B, S, X | Bypass, solo or stop the selected layer. |
| Del | Remove the selected clip. |
| Ctrl+I | Import media. |
| Ctrl+M | Map mode (MIDI and OSC); Esc cancels a learn, then leaves it. |
| Ctrl+Shift+A | Open the advanced output. |
| Space | Play or pause the timecode clock (internal source). |

(On macOS Ctrl reads as Cmd.)
