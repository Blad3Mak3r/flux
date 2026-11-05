---
name: Flux Timecode
description: A calibrated instrument screen for building timecode tracks; matte faceplate, etched graticule, phosphor traces.
colors:
  panel: "oklch(0.255 0.014 250)"
  panel-hi: "oklch(0.31 0.015 250)"
  panel-lo: "oklch(0.2 0.013 250)"
  rule: "oklch(0.36 0.015 250)"
  rule-strong: "oklch(0.46 0.018 250)"
  lamp-off: "oklch(0.34 0.012 250)"
  screen: "oklch(0.175 0.024 205)"
  graticule-minor: "oklch(0.255 0.032 205)"
  graticule-major: "oklch(0.36 0.05 205)"
  ghost: "oklch(0.235 0.03 205)"
  ink: "oklch(0.93 0.012 85)"
  ink-dim: "oklch(0.76 0.016 85)"
  ink-faint: "oklch(0.6 0.016 85)"
  phosphor: "oklch(0.87 0.17 128)"
  phosphor-dim: "oklch(0.6 0.12 128)"
  amber: "oklch(0.82 0.15 78)"
  amber-dim: "oklch(0.58 0.11 78)"
  signal: "oklch(0.7 0.2 32)"
typography:
  legend:
    fontFamily: "system-ui, -apple-system, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    lineHeight: 1.4
    letterSpacing: "0.14em"
  key:
    fontFamily: "system-ui, -apple-system, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "10px"
    fontWeight: 600
    letterSpacing: "0.1em"
  body:
    fontFamily: "system-ui, -apple-system, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.4
  title:
    fontFamily: "system-ui, -apple-system, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "15px"
    fontWeight: 700
  measure:
    fontFamily: "ui-monospace, 'SF Mono', 'Cascadia Mono', 'Roboto Mono', Menlo, Consolas, monospace"
    fontSize: "12px"
    fontWeight: 400
    fontFeature: "'tnum'"
rounded:
  control: "2px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  header-w: "208px"
  inspector-w: "300px"
components:
  key:
    backgroundColor: "{colors.panel-hi}"
    textColor: "{colors.ink}"
    typography: "{typography.key}"
    rounded: "{rounded.control}"
    height: "70px"
    width: "84px"
  key-pressed:
    backgroundColor: "{colors.panel-lo}"
  button:
    backgroundColor: "{colors.panel-hi}"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "32px"
    padding: "0 12px"
  button-primary:
    backgroundColor: "{colors.phosphor}"
    textColor: "{colors.panel-lo}"
    rounded: "{rounded.control}"
    height: "32px"
    padding: "0 12px"
  button-danger:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "32px"
  field:
    backgroundColor: "{colors.screen}"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "32px"
    padding: "0 8px"
  tab-selected:
    backgroundColor: "{colors.panel-lo}"
    textColor: "{colors.ink}"
    typography: "{typography.key}"
    height: "34px"
  clip:
    backgroundColor: "{colors.screen}"
    textColor: "{colors.phosphor}"
  cue-chip:
    backgroundColor: "{colors.screen}"
    textColor: "{colors.ink}"
    padding: "2px 10px 2px 8px"
  inspector-rail:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink-dim}"
    width: "40px"
  swatch:
    rounded: "0px"
    height: "24px"
    width: "24px"
  fader:
    backgroundColor: "{colors.screen}"
    textColor: "{colors.ink-dim}"
    height: "16px"
  level-meter:
    backgroundColor: "{colors.screen}"
    textColor: "{colors.ink-dim}"
    height: "10px"
---

# Design System: Flux Timecode

## Overview

**Creative North Star: "The Bench Instrument"**

The app is an oscilloscope-style faceplate, not a dashboard. A matte charcoal-blue panel carries silkscreened legends, square engraved keys and small state lamps; set into it is a deep teal-black screen etched with a graticule, on which time is read off a ruler and clips and cues are drawn as phosphor and amber traces. The scene is a booth or a stage, so the system is dark by commitment: there is no light theme and none should be added.

Time comes first. The timecode is a 5x7 dot-matrix readout at the top left of the faceplate, transport keys sit in the centre, output lamps at the right. Below, a ruler over graticule lanes with track legends on the left, and an inspector on the right. It is Operate mode: dense, measurable, quiet until something is live.

The system refuses the web-dashboard card layout and the grey DAW slab. There is no glow and there are no rounded cards. State never rides on colour alone: kind, selection, mute and lamp state each carry a second cue (fill pattern, outline, dash, position, text).

**Key Characteristics:**
- Two grounds only: faceplate (hue 250) and screen (hue 205); everything on the screen is read against the graticule.
- Colour is signal: phosphor reads, amber cues and holds, vermilion means live or fault.
- Squared forms, 2px control radius at most, 1px rules, lamps as squares.
- Dot-matrix digits snapped to whole device pixels.
- Layouts survive longer Spanish text (English default, Spanish shipped, no hard-coded strings).

## Colors

A restrained, near-monochrome faceplate with three signal hues used sparingly and by meaning.

### Primary
- **Phosphor Green** (`phosphor`): the reading colour. Dot-matrix lit pixels, audio clip traces, the ruler cursor and span, focus ring, selection, caret, active tab underline, primary button fill, "on" lamps.
- **Dim Phosphor** (`phosphor-dim`): clip borders, muted clip text, span-shade edges.

### Secondary
- **Cue Amber** (`amber`): cues (flag line), the paused hold word, run-info warnings. **Dim Amber** (`amber-dim`) outlines cue chips.

### Tertiary
- **Live Vermilion** (`signal`): live and fault state only: the PLAYING word, the playhead while playing, a live key lamp, fault lamps (including the clock lamp reading "Not real time" when the audio device does not keep time), invalid fields and error messages.

### Neutral
- **Faceplate** (`panel`), **Faceplate Raised** (`panel-hi`: keys, buttons, hover chrome), **Faceplate Recessed** (`panel-lo`: menubar, statusbar, pressed keys, ruler row), **Rule** and **Rule Strong** (`rule`, `rule-strong`: 1px and 2px dividers, control borders), **Lamp Off** (`lamp-off`).
- **Screen** (`screen`), **Graticule Minor/Major** (`graticule-minor`, `graticule-major`), **Ghost** (`ghost`: unlit dot-matrix pixels).
- **Ink** (`ink`, warm off-white), **Ink Dim** (`ink-dim`: legends, secondary text), **Ink Faint** (`ink-faint`: hints, hover line).

### Named Rules
**The Live-Only Vermilion Rule.** Vermilion appears only when something is live or faulted. The playhead is ink while stopped and turns vermilion only while playing.

**The Second Cue Rule.** No state is carried by colour alone: audio is a solid waveform, a marker is a hatched chip, a MIDI cue is a dotted chip; selection is a 2px ink outline; mute is dashed and hollow with a struck name; lamps also print a status word.

## Typography

**UI Font:** the system stack (`system-ui`, with Segoe UI, Roboto, Helvetica Neue, Arial fallbacks).
**Measure Font:** the system monospace stack (`ui-monospace`, SF Mono, Cascadia Mono, Roboto Mono, Menlo, Consolas), tabular numerals, used only for measurement: timecode readouts, ruler labels, zoom value, clip gain, numeric fields.
**Display:** the dot-matrix timecode window (SVG squares, see Components); it is drawn, not a font.

**Character:** plain and legible so the screen, not the type, carries the identity. Legends are small, bold, uppercase and widely tracked, as if printed on the panel.

**Decision on record:** the UI face is deliberately the system stack (Operate mode: the operator works inside it for hours, familiar native rendering is the point). The user accepted it explicitly. The finish review had left "self-host an engraved legend face" open pending that acceptance; it is closed, and can be reopened if the product later wants a printed-legend face.

### Hierarchy
- **Title** (700, 15px): empty-state title, dialog heading.
- **Body** (400/600, 13px, 1.4): track names, fields, lamp rows, inspector headings (700).
- **Status** (400, 12px): status bar, hints, field notes, readout line (uppercase, 0.06em).
- **Legend** (600, 11px, 0.14em, uppercase): section legends with a 1px rule running out of them; field labels use 0.08em; brand is 700 at 0.16em.
- **Key label** (600, 10px, 0.1em, uppercase): transport keys, tab labels (700, 11px).
- **Measure** (400, 11-12px, tabular): ruler labels, zoom value, readout line, gain.

### Named Rules
**The Measurement Only Rule.** Monospace is for things you measure. Names, labels and prose are never monospace.

## Layout

A full-height grid, `min-width: 960px` (minimum window 960x600), body never scrolls: rows are menubar (38px), faceplate, work area, statusbar (30px). The faceplate is three columns (readout, keys, lamps) with a 28px gap and 16px/20px padding. The work area is the screen (flexible) plus a 300px inspector that collapses to a 40px rail (see Inspector Toggle and Rail). The screen holds a sticky ruler row and sticky 208px track headers; the graticule is a background aligned to the end of the header column, major and minor lines set by timeline scale.

Spacing is a small hand-set rhythm: 4, 6, 8, 10, 12, 14, 16, 24px. Controls are 28px (icon and menubar buttons), 32px (fields and buttons), 34px (tabs, screen bar), 70px (keys).

Below 1180px: brand hidden, keys shrink to 70px minimum, lamp columns tighten, inspector narrows to 264px (the rail stays 40px), faceplate gap 18px, and the dot-matrix cell steps down from 9 to 7 CSS pixels. Text truncates with ellipsis rather than wrapping; fixed columns use `ch` or `em` widths so Spanish strings fit.

## Elevation & Depth

Flat and tonal. Depth is conveyed by faceplate tone (recessed `panel-lo`, base `panel`, raised `panel-hi`) and by 1-2px rules, never by blur. The only structural "shadow" is a 1px hard rule under the sticky ruler (`box-shadow: 0 1px 0 rule-strong`), which is a divider drawn as a shadow, not a lift. Keys read as physical by a 4px bottom border that flips to a 4px top border when pressed. The dialog backdrop is a flat 70% dark wash.

### Named Rules
**The No Glow Rule.** Nothing glows, blurs or casts a soft shadow. Phosphor is a flat fill.

## Shapes

Square and engraved. Controls use a 2px radius; keys, lamps, clips, cue chips, the readout window and the dialog are square. Lamps are 10px squares (key lamps 6px). The playhead has a small downward triangle head. Clip, flag and chip edges are 1px lines; the readout window is a 2px recessed border with a 1px outer outline. Icons are drawn for the app on a 16px grid, 1.5px stroke, square caps and mitred joins, as SVG.

## Components

### Transport Keys
Square, raised keys (84x70px, 4px bottom edge) with an icon above an uppercase 10px label and a 6px state lamp at top right. Pressed (`aria-pressed`) flips the edge and recesses the fill. Lamp: off = `lamp-off`; on = ink; live tone = vermilion; hold tone = amber. Hover lightens the fill over 120ms.

### Buttons
32px, 2px radius, raised fill. **Primary** is solid phosphor with recessed-dark text. **Danger** is a transparent ground with an ink outline (outlined where neutral buttons are filled). Menubar and icon buttons are transparent until hover (raised fill, rule border); a pressed icon button takes the recessed fill and a dim-ink border. Disabled: 45% opacity, not-allowed cursor.

### Window Controls
The native title bar is off (`decorations: false`), so the menubar is the title bar: its text and empty space drag the window (`data-tauri-drag-region`, double-click toggles maximize) and three flat 46px-wide buttons end it, full menubar height and square: minimize, maximize or restore, close. They are transparent in ink-dim until hover (raised fill); close inverts to ink on recessed-dark, never vermilion, because vermilion means live or fault. They exist only inside the desktop app. Closing with unsaved changes asks first, through the same dialog as New and Open.

### Fields
32px, screen-coloured ground, strong rule border, 2px radius. Numeric fields use the measure face. Invalid: vermilion border and a bold vermilion note. Checkboxes are 16px with phosphor accent.

### Tabs
The Setup dialog's page list is a vertical tab list: 34px rows, 600 weight, each page led by a 6px square lamp that lights phosphor on the selected one, which also takes the recessed fill, ink text and bold weight (a second cue besides colour). Up and Down move between pages. A page that is planned but not built (Video) is dimmed like a disabled control, stays in the list, and prints a square 1px-ruled "Coming soon" tag, so the reason is words and not colour; it cannot be selected and is skipped by the arrow keys.

### Inspector Toggle and Rail
A 34px square icon button on the inspector's left edge, at the start of its title row, collapses the sidebar; Ctrl+B (⌘B on macOS) does the same and the tooltip says so. It carries `aria-expanded` and a translated label ("Collapse the inspector" / "Expand the inspector"), and the chevron points the way the edge will move. The inspector holds one page, the selection (project and output settings live in Setup). Collapsed, it is a 40px rail with a 1px rule on its left, holding only the toggle. The screen takes the freed width, so the ruler, lanes and playhead reflow with no page scroll at the 960px minimum. The state is remembered between sessions (`localStorage`, failing soft to expanded).

### Output Lamps
Rows of a 10px square lamp, uppercase name, status word and a truncating detail. Off = `lamp-off`; on = phosphor; fault = vermilion. The status word is always printed. With several outputs of a kind the row summarises them ("2 outputs", the full list in the tooltip) and shows the worst state, so one missing device reads as Missing. Each row is a button that opens its page of Setup (Clock opens Audio, LTC and MTC their own list): the lamps are the live state, Setup is where it changes. Hover raises the row to `panel-hi` with a 1px rule.

### Timecode Readout (signature)
A recessed screen window holding a 5x7 dot-matrix face drawn as SVG squares; lit pixels `phosphor`, unlit `ghost`. The cell is a whole number of device pixels (9 CSS px wide, 7 narrow, at least 4 device px, gap at least 1 device px) so no pixel blends. A readout line beneath carries the state word (PLAYING vermilion, PAUSED amber, otherwise ink) in measure type.

### Ruler, Lanes, Clips and Cues (signature)
The ruler is SVG ticks and labels over the graticule with a phosphor cursor and span. Lanes have a centre line and a major-grid bottom rule. An audio clip is a solid phosphor waveform in a dim-phosphor bordered box with a label plate. A cue is a 1px amber flag line with a chip: hatched for marker, dotted for MIDI. Selected = 2px ink outline. A muted track draws clips hollow and dashed, flags dashed and 70% opaque, with the name struck through and a MUTED tag. The playhead is a 2px ink line (vermilion when playing) with a triangle head.

### Track Colour
A track may carry its own colour so many tracks can be told apart. It is picked in the Selection inspector, for the track of the selected clip or cue: eight square 24px swatches (teal, cyan, blue, violet, magenta, rose, lime, steel) and a Default button that returns the track to phosphor (audio) or amber (events). The chosen swatch takes a 2px ink outline and a dark tick, and every swatch has a spoken name, so the choice never rides on colour alone. All eight sit at oklch lightness 0.8, which gives 9.6:1 or more against `screen`, and none is near vermilion. The colour is stored in `project.json` as `#rrggbb`, changes through the undo history, and is applied through `--trace` and `--trace-dim` on the track's header and lane: clip border, tint and waveform, the cue line, chip border and hatch or dots, and the kind icon in the header. Kind still reads from the fill (solid waveform, hatched or dotted chip), and a muted track is drawn hollow and dashed in the dim version of its colour.

### Fader and Level Meter
Both sit in an audio track's header under the name row, which is why an audio lane is 96px tall. The fader is a recessed rail with a 1px notch at 0 dB and a square ink thumb (phosphor on hover), the value in measure type to its right; range -inf to +6 dB, 1 dB per arrow key, 0.1 with Shift, 6 per Page key, double-click returns to 0 dB. The meter is 24 square segments of 2.5 dB on the screen ground, off = `lamp-off`, lit = phosphor up to -12 dBFS, amber to -3, vermilion above. Flat fills only, no glow, no gradient. The peak-hold marker is one lone lit segment above the bar. A printed scale (-48, -24, -12, 0 with ticks between) sits below, and the accessible value says the level in words, so colour never carries the reading alone. The CLIP lamp is printed text that fills vermilion when the peak reached 0 dBFS and stays until pressed. The bar rises at once and falls at 24 dB/s, the marker holds 1.5 s then falls at 12 dB/s; with reduced motion the bar follows the signal and the marker jumps instead of sliding.

### Status Bar and Dialog
30px recessed bar: message left (vermilion bold on error), tabular measurements right. The unsaved dialog is a square panel, max 420px, with a flat backdrop.

### Setup Dialog
Everything that is set up rather than played lives in one modal opened from the menubar's Setup button, Ctrl+, (⌘, on macOS) or a faceplate lamp. A native `<dialog>`, 760x520 (92vw / 88vh at most), square, on the dialog's flat backdrop: a 44px recessed title row with a close button, a 200px page list on the left (Project, Audio, LTC outputs, MTC outputs, General, Video), the page on the right, and a recessed footer that prints what the output is doing (running device and rate, "Changes apply when playback stops", and any warnings in amber). It is modal because setup is rare, holds lists that would crowd the 300px inspector, and the transport keys must not fire behind it; the faceplate keeps the live state in view. Output lists are rows between 1px rules, not cards: a name (LTC 1), a lamp with its status word, a remove button, and below them the device or port select and, for LTC, the channel select. Outputs connect by themselves when a choice is made; a change made while playing waits for stop or pause.

## Do's and Don'ts

### Do:
- **Do** keep the product dark-only; scene is booth or stage.
- **Do** use phosphor for reading, amber for cues and hold, vermilion for live and fault only.
- **Do** tell every kind and state apart by a second cue as well as colour (fill pattern, outline, dash, printed word).
- **Do** keep corners square (2px at most) and rules at 1px, 2px for plate edges.
- **Do** keep the dot-matrix cell on whole device pixels.
- **Do** put every user-facing string through i18n (English and Spanish) and test layouts with the longer Spanish text.
- **Do** keep monospace to measurement and tabular numerals on all time values.
- **Do** keep the existing themed selection, caret, 2px phosphor focus ring, scrollbars and reduced-motion handling on new surfaces.

### Don't:
- **Don't** use rounded cards, soft shadows, blur or glow.
- **Don't** add a light theme.
- **Don't** use vermilion for decoration, emphasis or warnings that are not live or fault.
- **Don't** let colour alone carry kind, selection, mute or lamp state.
- **Don't** set names, labels or prose in monospace.
- **Don't** drift toward a dashboard card grid or a grey DAW slab.

## Open Items

Recorded honestly as unresolved, not as rules:
- The CLIP lamp and the top meter segments use vermilion although the transport is not live: an over-0 dBFS peak is treated as a fault, since the signal exceeded what the device can play. Revisit if vermilion should stay reserved for the transport.
- The waveform is a labelled placeholder, not real audio peaks.
- There is no add-track control; tracks are created by importing audio or pressing M.
- No engraved legend face is shipped; the user accepted the system stack. Reopen only if the product wants a printed-legend face later.
- The seed reference in the contract (`97c645d5`) is the raw key the roll printed, kept as printed.
- Minor build drift: a few component fills repeat token values as literals (key and button hover `oklch(0.35 0.016 250)`, primary hover `oklch(0.92 0.17 128)`, clip tints); run-info warnings use amber outside the cue/hold meaning.
