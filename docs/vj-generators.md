# VJ generators

A generator is a clip whose picture FLUX makes itself, with no video file: a solid colour, a strobe, a gradient, colour
bars, noise or a block of text. Generators fill the composition, take part in the grid like any clip (launch, layer
blend, [effects](vj-effects.md), transport) and are stored with the project. See the [VJ mode guide](vj-mode.md) for the
grid and [VJ blend modes](vj-blend-modes.md) for how layers mix.

To use one, open the **Generators** tab of the media browser and drag a generator onto a cell of the grid. Select the
clip to edit its settings in the inspector. Each entry below is headed by the generator's name and its file name in
backticks, `like this`.

| Generator | Draws |
| --- | --- |
| [Solid Color](#solid-color-solid) | One colour over the whole composition. |
| [Strobe](#strobe-strobe) | Flashes of a colour. |
| [Gradient](#gradient-gradient) | A blend from one colour to another. |
| [Color Bars](#color-bars-bars) | The bars of a video test card. |
| [Noise](#noise-noise) | Random snow. |
| [Text Block](#text-block-text) | Lines of text over a clear background. |

All colours are `#rrggbb`.

## Solid Color · `solid`

One colour over the whole composition.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Colour | any colour | #ffffff | The colour. |

## Strobe · `strobe`

Flashes of a colour. Between flashes the layer shows nothing, so what is below shows through.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Colour | any colour | #ffffff | The flash colour. |
| Rate | 0.01 to 30 Hz | 8 Hz | Flashes a second. |
| Flash | 0.01 to 1 | 0.5 | How long each flash lasts, as a share of the time between two flashes. |

## Gradient · `gradient`

A blend from one colour to another across the composition.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| From | any colour | #000000 | The colour at the start. |
| To | any colour | #ffffff | The colour at the end. |
| Angle | -360 to 360° | 0° | Direction of the gradient. 0 is left to right, 90 is top to bottom. |

## Color Bars · `bars`

The seven bars of a video test card, left to right: white, yellow, cyan, green, magenta, red and blue. It has no
settings.

## Noise · `noise`

Random snow in one colour.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Colour | any colour | #ffffff | The colour of the snow. |
| Rate | 0.01 to 30 Hz | 30 Hz | New snow a second. |

## Text Block · `text`

Lines of text over a transparent background, so what is below shows through, as in Resolume's Text Block.

| Setting | Range | Default | Does |
| --- | --- | --- | --- |
| Text | up to 4000 characters | Text | The text. Enter starts a new line. It is applied when you leave the box or press Ctrl+Enter. |
| Font | any installed family | Inter | The font family. |
| Style | Regular, Bold, Italic, Bold Italic | Bold | The face of the family. |
| Size | 0.01 to 2 | 0.12 | Height of the font as a share of the composition's height. |
| Colour | any colour | #ffffff | The colour of the letters. |
| Opacity | 0 to 1 | 1 | How solid the text is. |
| Align | Left, Center, Right | Center | How the lines sit against each other. |
| Position X | -1 to 2 | 0.5 | Centre of the block, as a share of the composition's width. 0.5 is the middle. |
| Position Y | -1 to 2 | 0.5 | Centre of the block, as a share of the composition's height. |
| Rotation | -360 to 360° | 0° | Turn of the block, clockwise. |
| Spacing X (tracking) | -0.5 to 2 em | 0 | Extra space between letters. |
| Spacing Y (leading) | -0.5 to 2 em | 0 | Extra space between lines. |
| Lines | All, One | All | All shows every line. One shows one line at a time. |
| Line | 1st line onwards | first | With Lines on One, the line shown, or the one the advance starts from. |
| Advance | Off, 1, 2, 4, 8 or 16 beats | Off | With Lines on One, steps to the next line every so many beats of the grid's clock. Off is by hand. |

### Fonts and fallback

- The font and style pickers list what is installed on the machine.
- If the family or the face is missing on the machine that plays the show, the text is drawn in the bundled **Inter**
  (Regular and Bold, Latin), so a text always draws.
- A character the chosen face lacks (Cyrillic, Chinese, Japanese, Korean, symbols, emoji) is drawn with another
  installed face that has it, and failing that with Inter. A character nobody has shows the chosen face's own
  missing-character box.
- Letters are shaped properly: kerning, ligatures and combining marks work. Ligatures are switched off while Spacing X
  is not 0, so spaced letters stay apart.
- A colour emoji that a font only has as bitmaps (Noto Color Emoji, Apple Color Emoji) leaves a blank of its width.
  Fonts that carry emoji as outlines draw them in the block's colour, never in their own colours.
- Tabs become spaces, and other control characters are dropped.

### Position, rotation and spacing

Position X and Y place the centre of the whole block, so the text can be moved off screen (down to -1 and up to 2 of the
composition). Rotation turns the block about its centre. Spacing X (tracking) and Spacing Y (leading) are counted in
ems, a unit of the font's size, so they scale with Size.

### One line at a time

With **Lines** on **One** a text block shows a single line of its text.

- **Previous** and **Next** buttons step the starting line by hand. Empty lines are skipped and the line numbers wrap
  round. The choice is part of the project (it enters the undo history), so a saved show opens where it was left.
- **Advance** steps to the next line automatically every 1, 2, 4, 8 or 16 beats of the grid's clock, counted from the
  line set by hand. Stepping on the grid's beats rather than on the moment of the launch keeps the changes on the beat.
  It goes round the lines that are not empty.
- A **readout** shows `shown / total` for a playing clip: the line on screen now and how many lines hold text. While the
  clip is not playing it shows the line you have set.

A text block with no text (or only blanks) draws nothing.

## Duration of generators and stills

A generator, like a still image, has no file with a length, so its clip has a **Duration**, set in the Transport
section of the Clip inspector. It is the length the clip's transport (region, cues, loop, once, tempo) runs over.

| Setting | Range | Default |
| --- | --- | --- |
| Duration | 0.1 s to 1 h (3600 s) | 10 s |

With the clip set to loop, the clip restarts after the Duration (a strobe and noise start their phase over); set to
once, the layer shows nothing after it. The countdown on the cell counts down what is left of it. See
[clip transport](vj-mode.md#clip-transport).

## Modulating generator numbers

The "~" button next to a number drives it with an LFO or the sound (see [modulation](vj-mode.md#modulation)). The
numbers that can be modulated:

| Generator | Modulatable |
| --- | --- |
| Solid Color | none |
| Strobe | Rate, Flash |
| Gradient | Angle |
| Color Bars | none |
| Noise | Rate |
| Text Block | Size, Opacity, Position X, Position Y, Rotation, Spacing X, Spacing Y |

Colours, the text itself, the font, style, alignment, Lines, Line and Advance cannot be modulated. A modulated value
stays within the range of its setting. Moving values are rounded to about a pixel, so a slow value costs nothing; very
fast modulation of Size, Spacing X or Spacing Y makes the text be drawn again at each step and is the most expensive
case.
