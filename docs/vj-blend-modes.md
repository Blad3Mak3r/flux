# VJ blend modes

A layer's **blend mode** says how its picture mixes with what the layers below show. Pick it in the layer strip's blend
menu (or the Layer inspector). FLUX has 29, grouped as the menu groups them. See the [VJ mode guide](vj-mode.md) for the
grid. Each entry is headed by the blend's name and its file name in backticks, `like this`.

## How layers are composited

- Layers are drawn from the bottom of the grid to the top, each one **over everything below it**.
- A layer's **cover** is its picture's alpha times its opacity (and its master and crossfader side). Where the picture is
  clear (a Text Block's background, a PNG's transparent pixels, an empty layer) the light below stays as it is.
- The result is `result = mix(below, blend(below, layer), cover)`: the blend mode decides `blend`, and the cover decides
  how much of it shows. At 0 cover nothing changes, at full cover the blend shows whole.
- Blending is done in **gamma-encoded (sRGB) values**, as Resolume does, so Multiply, Overlay, Screen and the opacity
  fades look the same as there.
- A layer that is not Alpha never hides the layers below it.

Notation: `b` is the light below and `s` the layer's colour, each channel from 0 to 1 and worked out for red, green and
blue separately unless said otherwise. `luma(c) = 0.3 R + 0.59 G + 0.11 B`.

## General

### Alpha · `alpha`

The layer covers what is below. This is the normal mode.

`blend = s`

## Darken

### Darken · `darken`

Keeps the darker of the two.

`blend = min(b, s)`

### Multiply · `multiply`

Darkens what is below by the layer's colour. White leaves it, black blacks it.

`blend = b × s`

### Color Burn · `colorBurn`

Darkens what is below by the layer, with more contrast than Multiply.

`blend = 1 - min(1, (1 - b) / s)` (1 where `b` is 1, 0 where `s` is 0)

## Lighten

### Lighten · `lighten`

Keeps the lighter of the two.

`blend = max(b, s)`

### Screen · `screen`

Lightens what is below, softer than Add. Black leaves it, white whitens it.

`blend = b + s - b × s`

### Color Dodge · `colorDodge`

Brightens what is below by the layer, towards overexposure.

`blend = min(1, b / (1 - s))` (0 where `b` is 0, 1 where `s` is 1)

## Contrast

### Overlay · `overlay`

Multiplies where what is below is dark, screens where it is light.

Hard Light with the two swapped: `blend = hardlight(s, b)`.

### Soft Light · `softLight`

A gentle contrast, driven by the layer.

With `d = ((16b - 12)b + 4)b` when `b` is 0.25 or less, otherwise `d = sqrt(b)`:
`blend = b - (1 - 2s) × b × (1 - b)` when `s` is 0.5 or less, otherwise `blend = b + (2s - 1)(d - b)`.

### Hard Light · `hardLight`

Overlay driven by the layer's light: multiplies where the layer is dark, screens where it is light.

`blend = 2 × b × s` when `s` is 0.5 or less, otherwise `1 - 2(1 - b)(1 - s)`.

### Linear Light · `linearLight`

Burns or dodges linearly by the layer's light.

`blend = clamp(b + 2s - 1, 0, 1)`

### Vivid Light · `vividLight`

Burns or dodges by the layer's light, with more contrast than Linear Light.

Color Burn with `2s` when `s` is 0.5 or less, otherwise Color Dodge with `2s - 1`.

### Pin Light · `pinLight`

Keeps the darker or the lighter of the two, by the layer's light.

`blend = min(b, 2s)` when `s` is 0.5 or less, otherwise `max(b, 2s - 1)`.

## Comparative

### Add · `add`

Adds the layer's light to what is below.

`blend = min(1, b + s)`

### Subtract · `subtract`

Takes the layer's light from what is below.

`blend = max(0, b - s)`

### Difference · `difference`

The distance between the two lights. Identical pictures give black.

`blend = |b - s|`

### Difference I · `differenceInverted`

The inverse of Difference, a lighter picture. Identical pictures give white.

`blend = 1 - |b - s|`

### Exclusion · `exclusion`

Like Difference with less contrast.

`blend = b + s - 2 × b × s`

## Colour

These four work on the whole colour instead of each channel. They use the standard hue, saturation and luminosity
formulas of the W3C Compositing and Blending specification, with the luma weights above (`Lum` is `luma`, `Sat` is the
spread between the largest and the smallest channel, `SetSat` and `SetLum` set them).

### Hue · `hue`

The layer's hue on the saturation and luminosity below.

`blend = SetLum(SetSat(s, Sat(b)), Lum(b))`

### Saturation · `saturation`

The layer's saturation on the hue and luminosity below.

`blend = SetLum(SetSat(b, Sat(s)), Lum(b))`

### Color · `color`

The layer's hue and saturation on the luminosity below.

`blend = SetLum(s, Lum(b))`

### Luminosity · `luminosity`

The layer's luminosity on the hue and saturation below.

`blend = SetLum(b, Lum(s))`

## Keys and masks

These change how much of the layer shows, not just how it looks. The cover is `alpha × opacity`.

### Alpha Cut · `alphaCut`

A hard cut by the layer: where its cover is at least half it replaces what is below, elsewhere it leaves it.

`blend = s`, shown at full strength where `cover ≥ 0.5` and not at all elsewhere.

### Luma Key · `lumaKey`

The layer's dark parts turn transparent.

`blend = s`, shown by `cover × luma(s)`.

### Luma Key I · `lumaKeyInverted`

The layer's bright parts turn transparent.

`blend = s`, shown by `cover × (1 - luma(s))`.

### Luma is Alpha · `lumaIsAlpha`

The layer's brightness replaces its own alpha: bright is opaque, dark is transparent, wherever the layer has a picture.

`blend = s`, shown by `opacity × luma(s)` wherever `cover` is above 0 (the layer's own alpha is ignored).

### 50 Mask · `mask50`

The layer acts as a mask and is not drawn itself: what is below shows where the layer is bright and goes dark where it is
dark.

`blend = b × luma(s)`, by `cover`. White keeps what is below, black blacks it out, grey halves it.

### RGB · `rgb`

The layer takes over what is below one channel at a time as its cover rises: blue first, then green, then red.

`blend = s`, with the cover per channel `blue = clamp(3 × cover, 0, 1)`, `green = clamp(3 × cover - 1, 0, 1)`,
`red = clamp(3 × cover - 2, 0, 1)`. Up to a third of cover only the blue changes, up to two thirds the green joins,
and the red last.

### Displace · `displace`

The layer shifts what is below sideways by its brightness, and is not drawn itself.

What is below is read `(luma(s) - 0.5) × 20%` of the composition's width to one side: white pushes it left, black
pushes it right, mid grey leaves it. The shift is rounded to a whole pixel and cut at the edges of the picture. The
result is mixed in by `cover`.

## Transitions with a blend mode

A layer's **transition mode** (the menu under the **T** fader) takes the same 29 blend modes. **Alpha**, the default, is a
plain crossfade: the new clip fades in over the old one, which stays whole until it is covered. With any other mode the
new clip is blended with the old one by that mode as the transition runs, the way Resolume's transition modes work. A
transition mode other than Alpha is worked out like this, with `t` the progress of the transition from 0 to 1:

- the old clip is drawn over a clear picture at `1 - t` of its cover;
- the new clip is drawn over it with the transition's blend mode, at `t` of its cover;
- that mix is then drawn over the layers below with the layer's own blend mode, opacity and master, as one picture.

At `t = 0` only the old clip shows and at `t = 1` only the new one, so the cut at the end of the transition does not
jump. In between, **Add** adds the two lights, **Multiply** darkens where both are dark, **Difference** lights up where
they differ, and so on for every mode. Resolume also has transition effects that are not blend modes (wipes, slides and
the like in its transition menu, and a Random choice); FLUX does not have them yet.

A layer that is in a [group](vj-mode.md#groups) cannot nest a second mix, so there the new clip is blended over the old
one flat, with the transition's blend, and the group mixes the result.

## Choices where Resolume is silent

Resolume's manual describes these modes in a line each. Where it leaves the detail open, FLUX makes these choices:

| Mode | Choice |
| --- | --- |
| Displace | The shift is up to **20% of the width** either way, at full white or black. Resolume's page gives no amount. |
| RGB | Blue is taken over first, then green, then red, each over a third of the cover. |
| Alpha Cut | The cut is at **half** cover. |
| Luma Key, Luma Key I, Luma is Alpha, 50 Mask, Displace | Brightness is the standard weighted sum `0.3 R + 0.59 G + 0.11 B`, taken on the gamma-encoded colour. |
| Luma is Alpha | The layer's opacity times its brightness replaces the cover; the layer's own alpha is ignored but a fully clear pixel (no picture) stays clear. |
| 50 Mask | Multiplies the light below by the brightness of the layer. Nothing of the layer itself is drawn. |
| Difference I | One minus the distance between the two lights, per channel. |
