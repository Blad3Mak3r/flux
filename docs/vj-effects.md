# VJ effects

An effect changes the picture of a clip, a layer or the whole composition. FLUX has 39 effects in seven categories,
listed here as the **Effects** tab of the media browser groups them. For the rest of VJ mode see the
[VJ mode guide](vj-mode.md); for the pictures FLUX makes itself see [VJ generators](vj-generators.md).

Each entry is headed by the effect's name and its file name in backticks, `like this`, which is what appears in the
project file.

## Effect stacks

- Every clip, every layer and the composition has its own **stack** of effects, shown under **Effects** in the
  inspector's Clip, Layer and Composition tabs.
- A stack holds up to **16** effects. They apply from the top down. A clip's stack applies first, then its layer's, so a
  layer's effects reach every clip on it, including both clips while a transition runs.
- Add an effect by dragging it from the browser onto a grid clip, a layer header, an inspector stack, the Output panel
  or the composition bar. Double click adds it to the selected clip (or layer, or to the composition while the
  Composition tab is shown). **Add effect** in a stack picks from a list grouped by category.
- In the stack each effect can be **bypassed** (it keeps its place and settings), folded shut, duplicated, reset to its
  starting settings, moved with its grip or the arrows, and removed. A badge on the clip and the layer header counts
  the effects.
- On the composition, a switch next to the **FX n** badge bypasses the whole stack at once, for the stage.
- The order matters for most effects. A few kinds are worked out in fixed groups (all the distortions, then the blurs
  and colour splits, then the colour effects), so a blur placed below a threshold still blurs first.

### Where an effect works

| Where | Which effects |
| --- | --- |
| Clip, layer or composition | Every effect except Feedback, Trails and Bloom. |
| Composition only | Feedback, Trails and Bloom. They need the frames before the current one, or the pixels around each pixel. A clip or a layer refuses them. |

The composition's stack applies to the final mix of all the layers, before the output mapping.

### Settings

- Settings counted in **%** are stored from 0 to 1 and shown as percentages. The ranges below are as the inspector shows
  them.
- A setting that is a number can be **modulated** (the "~" button next to it): an LFO or the sound moves it between a
  minimum and a maximum you choose. See [modulation](vj-mode.md#modulation). Choices, colours and switches cannot be
  modulated. A whole-number setting (segments, columns, rows, levels, lines) is rounded as it moves. The pace settings
  (Rate and Cycle) are not modulated either.
- **X** and **Y** settings that are a centre run from 0 (left or top) to 100% (right or bottom); 50% is the middle.

### Pace

Effects that move with time share three settings. A cycle is what the effect says it is (a turn of a spin, a period of a
wave, a step of a glitch).

| Setting | Range | Does |
| --- | --- | --- |
| Sync | Free, Beats | Free counts cycles in Hz. Beats locks one cycle to a length in beats of the show's tempo, so it follows the BPM and TAP. |
| Rate | 0 to 30 Hz | Cycles a second when Sync is Free. 0 stands still. |
| Cycle (beats) | 1/4, 1/2, 1, 2, 4, 8 or 16 beats | Length of one cycle when Sync is Beats. |

Every animated effect below starts on Free. Its starting Rate and Cycle are given in its entry.

## Colour

### Brightness · `brightness`

Adds or removes light from every colour.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | -100 to 100% | 0% | Light added to every colour. 0 changes nothing. |

### Contrast · `contrast`

Spreads the colours away from mid grey, or squeezes them towards it.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 400% | 100% | Above 100% increases contrast, below flattens towards grey. 100% changes nothing. |

### Saturation · `saturation`

From black and white to vivid colour.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 400% | 100% | 0% is grey, above 100% pushes the colours further from their grey. 100% changes nothing. |

### Hue · `hue`

Turns every colour round the colour wheel.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Angle | -180 to 180° | 0° | How far the colours turn. 0 changes nothing. |

### Tint · `tint`

Washes the picture with one colour.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Colour | any colour | #ff8800 | The tint. |
| Amount | 0 to 100% | 50% | How far the picture's grey goes towards the colour. |

### Invert · `invert`

Turns the picture into its negative.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 100% | How far the colours go towards their negative. |

### Levels · `levels`

Sets the black and white points of the picture and bends its mid tones.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Black | 0 to 100% | 0% | The level that becomes black. |
| White | 0 to 100% | 100% | The level that becomes white. |
| Gamma | 0.1 to 4 | 1 | Bends the mid tones. 1 changes nothing. |

### Colorize · `colorize`

Paints the picture's greys from a dark colour to a light one (a gradient map).

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Dark | any colour | #1a0b3d | The colour of the darkest parts. |
| Light | any colour | #ffb347 | The colour of the lightest parts. |
| Amount | 0 to 100% | 100% | How far the picture goes towards the painted version. |

### Solarize · `solarize`

Turns the light tones into their negative.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Threshold | 0 to 100% | 50% | Colours above this level are inverted. |

## Distort

### Mirror · `mirror`

Folds one half of the picture onto the other.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Axis | Horizontal, Vertical, Both | Horizontal | Horizontal copies the left half onto the right, Vertical the top onto the bottom, Both the top left quarter onto the other three. |

### Zoom · `zoom`

Draws the picture larger or smaller about its centre. What falls outside the picture is transparent.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 25 to 800% | 100% | Size of the picture. 100% changes nothing. |

### Shift · `shift`

Moves the picture. What leaves one side comes back on the other.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Horizontal | -100 to 100% | 0% | Distance to the right, as a share of the picture's width. |
| Vertical | -100 to 100% | 0% | Distance down, as a share of its height. |

### Rotate · `rotate`

Turns the picture, still or spinning. The corners it leaves are transparent.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Angle | -180 to 180° | 0° | Fixed turn. |
| Spin (turns/cycle) | -4 to 4 | 0 | Turns added every cycle. Negative spins the other way. |
| Pace | | Free, 0.25 Hz, 4 beats | One cycle is `Spin` turns. |

### Kaleidoscope · `kaleidoscope`

Repeats a wedge of the picture round a point.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Segments | 2 to 32 | 6 | Number of wedges. |
| Rotation | -180 to 180° | 0° | Fixed turn of the wedges. |
| Spin (turns/cycle) | -4 to 4 | 0.25 | Turns added every cycle. |
| Horizontal | 0 to 100% | 50% | Centre of the pattern. |
| Vertical | 0 to 100% | 50% | Centre of the pattern. |
| Pace | | Free, 0.5 Hz, 4 beats | One cycle is `Spin` turns. |

### Polar · `polar`

Bends the picture from a rectangle into a disc.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 100% | How far it bends. |

### Wave Warp · `wave`

Ripples the picture with travelling waves.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amplitude | 0 to 25% | 3% | How far the picture is pushed, as a share of its size. |
| Waves | 0 to 32 | 4 | Number of waves across the picture. |
| Direction | Horizontal, Vertical, Both | Horizontal | Which way it waves. |
| Pace | | Free, 0.5 Hz, 1 beat | One cycle is one wave travelling by. |

### Twirl · `twirl`

Spins the picture about a point, most at the centre and not at all at the edge of the radius.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Angle | -1440 to 1440° | 180° | Turn at the centre. |
| Radius | 0 to 200% | 50% | Reach of the twirl, in picture heights. |
| Horizontal | 0 to 100% | 50% | Centre of the twirl. |
| Vertical | 0 to 100% | 50% | Centre of the twirl. |

### Bulge · `bulge`

Swells or pinches the picture round a point, like a fisheye.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | -100 to 100% | 50% | Positive swells, negative pinches. |
| Radius | 5 to 200% | 60% | Reach of the effect. |
| Horizontal | 0 to 100% | 50% | Centre of the bulge. |
| Vertical | 0 to 100% | 50% | Centre of the bulge. |

### Tile · `tile`

Repeats the picture in rows and columns.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Columns | 1 to 16 | 2 | Tiles across. |
| Rows | 1 to 16 | 2 | Tiles down. |
| Mirror alternate tiles | on / off | on | Turns every other tile over. |

### Pixelate · `pixelate`

Breaks the picture into big square pixels.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Cell size | 0 to 25% | 2% | Size of a cell, as a share of the picture's height. |

### Displace · `displace`

Pushes the picture about with drifting noise.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amplitude | 0 to 25% | 3% | How far the picture is pushed, as a share of its size. |
| Scale | 0.5 to 32 | 6 | Number of blobs of noise across the picture. |
| Pace | | Free, 0.5 Hz, 2 beats | One cycle is one step of the noise's drift. |

## Blur and edges

### Blur · `blur`

Softens the picture.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Radius | 0 to 100% | 10% | Share of the largest blur. |

### Radial Blur · `radialBlur`

Smears the picture round a point.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 30% | Strength of the smear. |
| Horizontal | 0 to 100% | 50% | Centre of the smear. |
| Vertical | 0 to 100% | 50% | Centre of the smear. |

### Zoom Blur · `zoomBlur`

Smears the picture along rays from a point.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 30% | Strength of the smear. |
| Horizontal | 0 to 100% | 50% | Centre of the rays. |
| Vertical | 0 to 100% | 50% | Centre of the rays. |

Blur, Radial Blur and Zoom Blur in one stack add up (two blurs combine to a little more than either, never past a full
blur).

## Glitch

### Shift Glitch · `shiftGlitch`

Shifts random bands of the picture sideways and now and then splits their colours. The bands change every cycle.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 15% | How far the bands shift. |
| Band height | 0.5 to 50% | 6% | Height of a band, as a share of the picture. |
| Probability | 0 to 100% | 35% | Chance that a band shifts in a cycle. |
| Pace | | Free, 8 Hz, 1/2 beat | One cycle is one new set of bands. |

### Twitch · `twitch`

Every cycle, with some probability, the picture jumps in position and size and flashes in light at random.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 50% | Size of the jump. |
| Probability | 0 to 100% | 50% | Chance of a jump in a cycle. |
| Pace | | Free, 4 Hz, 1/2 beat | One cycle is one chance to jump. |

### RGB Split · `rgbSplit`

Pulls the red and the blue of the picture apart.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 30% | How far apart. |
| Angle | -180 to 180° | 0° | Direction of the split. |
| Animate | on / off | off | Turns the direction one full turn every cycle. |
| Pace | | Free, 0.5 Hz, 4 beats | Used while Animate is on. |

## Stylise

### Posterize · `posterize`

Cuts every colour to a few steps.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Levels | 2 to 32 | 4 | Steps per colour. |

### Threshold · `threshold`

Turns the picture into two colours.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Level | 0 to 100% | 50% | Greys below it become Dark, above it Light. |
| Softness | 0 to 100% | 5% | Width of the transition between the two. |
| Dark | any colour | #000000 | Colour of the dark side. |
| Light | any colour | #ffffff | Colour of the light side. |

### Edge Detect · `edgeDetect`

Draws the outlines of the picture.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Strength | 0 to 8 | 1 | How bright the edges are. |
| Mode | Edges, Glow | Edges | Edges shows only the edges, light on black. Glow adds them to the picture. |

### Vignette · `vignette`

Darkens the corners of the picture.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 60% | How dark the corners get. |
| Softness | 0 to 100% | 50% | How gradual the fall-off is. |
| Roundness | 0 to 100% | 50% | 0% follows the frame, 100% is an ellipse. |

### Film Grain · `noise`

Adds noise, still or moving.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Amount | 0 to 100% | 25% | Strength of the noise. |
| Grain size | 0 to 100% | 10% | How coarse the grain is. |
| Animated | on / off | on | The grain changes about 24 times a second. Off keeps it still. |

### Scanlines · `scanlines`

Dark lines across the picture, like a monitor.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Lines | 2 to 540 | 120 | Number of lines. |
| Intensity | 0 to 100% | 50% | How dark the lines are. |
| Pace | | Free, 0 Hz, 4 beats | Scrolls the lines by one line spacing a cycle. 0 Hz keeps them still. |

## Time

### Strobe · `strobe`

Flashes the picture or blacks it out in time.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Colour | any colour | #ffffff | The flash colour. |
| On time | 2 to 100% | 25% | Share of each cycle that the strobe is on. |
| Mode | Flash, Blackout | Flash | Flash shows the colour, Blackout shows black. |
| Pace | | Free, 4 Hz, 1/2 beat | One cycle is one flash. |

### Slide · `slide`

Scrolls the picture without stopping.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Horizontal | -400 to 400% | 25% | Distance scrolled to the right every cycle, as a share of the picture's width. |
| Vertical | -400 to 400% | 0% | Distance scrolled down every cycle. |
| Wrap around | on / off | on | On, what leaves one side comes back on the other. Off leaves the rest transparent. |
| Pace | | Free, 1 Hz, 4 beats | One cycle is one scroll by the distances above. |

### Roll · `roll`

Rolls the picture like a TV out of vertical hold.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Jitter | 0 to 100% | 20% | How much the lines shake. |
| Pace | | Free, 0.3 Hz, 4 beats | One cycle is one roll of the picture. |

## Feedback (composition only)

These three only exist on the composition's stack and are tagged *Composition* in the browser. Dropping one on a clip
or a layer is refused; double clicking one adds it to the composition and opens that tab. They remember the frames
before: each output window keeps its own history, so two screens show two independent tunnels that start from black when
the window opens. The history is forgotten when the composition changes size and when the stack is empty or bypassed.

Their strength settings are counted per 1/60 of a second and their movements per second, so a result looks the same at 30
and at 120 frames a second.

### Feedback · `feedback`

Draws the last frame again, zoomed and turned: tunnels and spirals.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Feedback (per 1/60 s) | 0 to 100% | 60% | Share of the old frame kept every 1/60 s. 0 is no effect. |
| Zoom (per 1/60 s) | 0.5 to 2 times | 1.02 | How much larger the old frame is drawn each time. Above 1 grows out of the middle, below 1 shrinks into it. |
| Rotate | -180 to 180°/s | 0 | Turns the old frame. |
| Drift horizontal | -100 to 100%/s | 0 | Moves the old frame right, as a share of its width a second. |
| Drift vertical | -100 to 100%/s | 0 | Moves the old frame down. |
| Hue drift | -180 to 180°/s | 0 | Turns the colours of the old frame. |
| Blend | Mix, Add, Lighten | Mix | Mix goes the Feedback share of the way to the old frame. Add adds the old frame times Feedback. Lighten keeps the lighter of the new frame and the old one times Feedback. |

### Trails · `trails`

Leaves a fading trail behind whatever moves.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Persistence (per 1/60 s) | 0 to 100% | 90% | Share of the previous output kept every 1/60 s. 0 is no effect. |
| Mode | Blend, Lighten, Add | Blend | Blend averages the new frame and the trail (never brighter). Lighten keeps the lighter of the two. Add adds the trail to the new frame. |

### Bloom · `bloom`

Spreads a glow around what is bright.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Threshold | 0 to 100% | 60% | Only what is brighter than this glows. |
| Intensity | 0 to 400% | 100% | How strongly the glow is added back. |
| Radius | 0 to 100% | 50% | How far the glow spreads. |

### Order on the composition

On the composition the effects keep the order of the stack within three groups, and the groups run in this order: the
history effects (Feedback, Trails), then every other effect, then Bloom. A Bloom is always the last thing to touch the
picture, and at most two Blooms count. A Feedback draws what the viewer saw, less its Bloom, so a glow does not pile up
into white.

## About the monitor

The Output panel in the editor draws the effects on the CPU, with two cheaper approximations (a lighter blur and a
smaller picture), so fine noise and one-pixel edges can differ a little from the screens. The screens are the truth.
