---
name: FLUX
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
  header-w: "248px"
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

# Design System: FLUX

## Overview

**Creative North Star: "The Bench Instrument"**

The public name is **FLUX**, always uppercase. The modular F and the FLUX wordmark form the app's brand mark, with an 8px gap and 13px bold system lettering at 0.16em tracking. The launcher uses 22px lettering and a 12px gap. The symbol is decorative beside the readable name; an interactive mark keeps the translated About label. The existing instrument palette, controls and app icon are retained.

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
- **Phosphor Green** (`phosphor`): the reading colour. Dot-matrix lit pixels, audio clip traces, the ruler cursor and span, selection, caret, active tab underline, primary button fill, "on" lamps.
- **Dim Phosphor** (`phosphor-dim`): clip borders, muted clip text, span-shade edges.

### Secondary
- **Cue Amber** (`amber`): notes (their start line), the standby cue, the paused hold word, run-info warnings. **Dim Amber** (`amber-dim`) outlines note boxes.

### Tertiary
- **Live Vermilion** (`signal`): live and fault state only: the PLAYING word, the playhead while playing, a live key lamp, fault lamps (including the clock lamp reading "Not real time" when the audio device does not keep time), invalid fields and error messages.

### Neutral
- **Faceplate** (`panel`), **Faceplate Raised** (`panel-hi`: keys, buttons, hover chrome), **Faceplate Recessed** (`panel-lo`: menubar, statusbar, pressed keys, ruler row), **Rule** and **Rule Strong** (`rule`, `rule-strong`: 1px and 2px dividers, control borders), **Lamp Off** (`lamp-off`).
- **Screen** (`screen`), **Graticule Minor/Major** (`graticule-minor`, `graticule-major`), **Ghost** (`ghost`: unlit dot-matrix pixels).
- **Ink** (`ink`, warm off-white), **Ink Dim** (`ink-dim`: legends, secondary text), **Ink Faint** (`ink-faint`: hints, hover line).

### Named Rules
**The Live-Only Vermilion Rule.** Vermilion appears only when something is live or faulted. The playhead is ink while stopped and turns vermilion only while playing.

**The Second Cue Rule.** No state is carried by colour alone: audio is a solid waveform and a note is a hatched box; selection is a 2px ink outline; mute is dashed and hollow with a struck name; lamps also print a status word.

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

A full-height grid, `min-width: 960px` (minimum window 960x600), body never scrolls: rows are menubar (38px), faceplate, work area, statusbar (30px). The faceplate is three columns (readout, keys, lamps) with a 28px gap and 16px/20px padding. The work area is the screen (flexible) plus a 300px inspector that collapses to a 40px rail (see Inspector Toggle and Rail). The screen holds a sticky ruler row and sticky 248px track headers; the graticule is a background aligned to the end of the header column, major and minor lines set by timeline scale.

Spacing is a small hand-set rhythm: 4, 6, 8, 10, 12, 14, 16, 24px. Controls are 28px (icon and menubar buttons), 32px (fields and buttons), 34px (tabs, screen bar), 70px (keys).

Below 1180px: brand hidden, keys shrink to 70px minimum, lamp columns tighten, inspector narrows to 264px (the rail stays 40px), faceplate gap 18px, and the dot-matrix cell steps down from 9 to 7 CSS pixels. Text truncates with ellipsis rather than wrapping; fixed columns use `ch` or `em` widths so Spanish strings fit.

## Elevation & Depth

Flat and tonal. Depth is conveyed by faceplate tone (recessed `panel-lo`, base `panel`, raised `panel-hi`) and by 1-2px rules, never by blur. The only structural "shadow" is a 1px hard rule under the sticky ruler (`box-shadow: 0 1px 0 rule-strong`), which is a divider drawn as a shadow, not a lift. Keys read as physical by a 4px bottom border that flips to a 4px top border when pressed. The dialog backdrop is a flat 70% dark wash.

### Named Rules
**The No Glow Rule.** Nothing glows, blurs or casts a soft shadow. Phosphor is a flat fill.

## Shapes

Square and engraved. Controls use a 2px radius; keys, lamps, clips, note boxes, the readout window and the dialog are square. Lamps are 10px squares (key lamps 6px). The playhead has a small downward triangle head. Clip and note edges are 1px lines; the readout window is a 2px recessed border with a 1px outer outline. Icons are drawn for the app on a 16px grid, 1.5px stroke, square caps and mitred joins, as SVG.

## Components

### Transport Keys
Square, raised keys (84x70px, 4px bottom edge) with an icon above an uppercase 10px label and a 6px state lamp at top right. Pressed (`aria-pressed`) flips the edge and recesses the fill. Lamp: off = `lamp-off`; on = ink; live tone = vermilion; hold tone = amber. Hover lightens the fill over 120ms.

### Buttons
32px, 2px radius, raised fill. **Primary** is solid phosphor with recessed-dark text. **Danger** is a transparent ground with an ink outline (outlined where neutral buttons are filled). Menubar and icon buttons are transparent until hover (raised fill, rule border); a pressed icon button takes the recessed fill and a dim-ink border. Disabled: 45% opacity, not-allowed cursor.

### Menus
The menubar holds five titles (File, Edit, Track, View, Transport) and a centred document name. A title opens a dropdown; once one is open, pointing at another opens it, and Left/Right move along the bar. Right click opens a context menu for what is under the pointer (audio clip or track, note, notes track, empty timeline, Live cue, text field); the webview's own menu is never shown. Both are the same flat panel: `panel-hi` ground, 1px strong rule, 2px radius, 28px rows, a 16px check column (a drawn tick marks an "on" item, so state is never colour alone), the shortcut right-aligned in the measure face. The focused or hovered row is recessed (`panel-lo`). A row that leads to a submenu (File > Import) carries a chevron in the shortcut column and opens the submenu flat against its right edge (to the left when the window has no room) while pointed at; Right or Enter opens it from the keyboard and Left returns to the row. Disabled rows dim to 45%, which is how a planned entry such as Import > Video reads. Up, Down, Home, End, Enter, Space and Esc work, and while a menu is open it takes the keyboard from the transport shortcuts.

### Focus
Keyboard focus is a 1px inset line in `ink-dim` (`outline-offset: -1px`) on every control, so it never spills past a border or fights a selection outline. It is not phosphor: phosphor is for reading.

### Window Controls
The native title bar is off (`decorations: false`), so the menubar is the title bar: its text and empty space drag the window (`data-tauri-drag-region`, double-click toggles maximize) and three flat 46px-wide buttons end it, full menubar height and square: minimize, maximize or restore, close. They are transparent in ink-dim until hover (raised fill); close inverts to ink on recessed-dark, never vermilion, because vermilion means live or fault. They exist only inside the desktop app. Closing with unsaved changes asks first, through the same dialog as New and Open.

### Fields
32px, screen-coloured ground, strong rule border, 2px radius. Numeric fields use the measure face. Invalid: vermilion border and a bold vermilion note. Checkboxes are 16px with phosphor accent.

### Tabs
The Preferences dialog's page list is a vertical tab list: 34px rows, 600 weight, each page led by a 6px square lamp that lights phosphor on the selected one, which also takes the recessed fill, ink text and bold weight (a second cue besides colour). Up and Down move between pages. A page that is planned but not built (Video) is dimmed like a disabled control, stays in the list, and prints a square 1px-ruled "Coming soon" tag, so the reason is words and not colour; it cannot be selected and is skipped by the arrow keys.

### Timeline Strip
A project holds several timelines (one per song or block of the show), shown as a strip of tabs in the 34px screen bar of both Edit and Live, between the legend and the controls. A tab is a 6px square lamp and the timeline's name, as in the Preferences page list: the chosen one takes the recessed fill, a 2px phosphor underline, ink text in bold and a lit phosphor lamp (several cues, never colour alone). The name is the user's text, so it is body type at 600 weight, never uppercase or monospace, and truncates with an ellipsis at 22ch (the full name in the tooltip) to survive Spanish. While the show plays, the active tab's lamp turns vermilion (it is on air) and the other tabs dim to 45% with a not-allowed cursor and a tooltip saying why: the backend refuses the change anyway. The strip scrolls sideways (wheel or touchpad, no scrollbar: a bar would eat the 34px) and the chevron button next to the `+` opens the whole list as a menu with the active one ticked, so any timeline is reachable. Arrow keys walk the focus along the tabs and Enter chooses one (arrows do not choose: choosing reopens the output). The tabs reorder by dragging, as the track headers do: past a 4px slop the tab follows the pointer along the strip, a 2px phosphor line marks where it will land, Escape cancels, and the click that ends a drag does not choose the tab; Move left and Move right in the tab's menu and the Timeline menu do the same without a pointer. While the show plays the order is locked, like choosing a timeline: no drag starts and the Move items are disabled. Ctrl+PageUp and Ctrl+PageDown step through the timelines and a Timeline menu holds the same actions; double click or right click on a tab offers Rename (Preferences, Timeline page) and Delete (the confirm dialog, with Cancel focused).

### Inspector Toggle and Rail
A 34px square icon button on the inspector's left edge, at the start of its title row, collapses the sidebar; Ctrl+B (⌘B on macOS) does the same and the tooltip says so. It carries `aria-expanded` and a translated label ("Collapse the inspector" / "Expand the inspector"), and the chevron points the way the edge will move. The inspector holds one page, the selection (project and output settings live in Preferences). Collapsed, it is a 40px rail with a 1px rule on its left, holding only the toggle. The screen takes the freed width, so the ruler, lanes and playhead reflow with no page scroll at the 960px minimum. The state is remembered between sessions (`localStorage`, failing soft to expanded).

### Output Lamps
Rows of a 10px square lamp, uppercase name, status word and a truncating detail. Off = `lamp-off`; on = phosphor; fault = vermilion. The status word is always printed. With several outputs of a kind the row summarises them ("2 outputs", the full list in the tooltip) and shows the worst state, so one missing device reads as Missing. Each row is a button that opens its page of Preferences (Clock opens Audio, LTC and MTC their own list): the lamps are the live state, Preferences is where it changes. Hover raises the row to `panel-hi` with a 1px rule.

### Timecode Readout (signature)
A recessed screen window holding a 5x7 dot-matrix face drawn as SVG squares; lit pixels `phosphor`, unlit `ghost`. The cell is a whole number of device pixels (9 CSS px wide, 7 narrow, at least 4 device px, gap at least 1 device px) so no pixel blends. A readout line beneath carries the state word (PLAYING vermilion, PAUSED amber, otherwise ink) in measure type.

### Ruler, Lanes, Clips and Cues (signature)
The ruler is SVG ticks and labels over the graticule with a phosphor cursor and span; only the ticks on screen (and a screenful each side) are drawn, so a long show at a deep zoom stays light. Lanes have a centre line and a major-grid bottom rule. An audio clip is a solid phosphor waveform in a dim-phosphor bordered box with a label plate. A note is a 1px amber line at its start with a hatched box beside it holding its first two lines of text; a note with a length is a box as long as it lasts. Selected = 2px ink outline. A clip's name can be typed in place (double-click the label; F2 renames the track) in a flat field that keeps the label's size and weight. A clip's fades are drawn as their real curve (linear, equal power, S-curve or exponential), and its volume envelope as a 1.5px phosphor line over the waveform: flat to both edges, with 8px square points on the selected clip (a drag shows its dB in the measure face; double-click adds or removes a point; the arrow keys move a focused one). A muted track draws clips hollow and dashed, notes dashed and 70% opaque, with the name struck through and a MUTED tag. The playhead is a 2px ink line (vermilion when playing) with a triangle head.

### Track Colour
A track may carry its own colour so many tracks can be told apart. It is picked in the Selection inspector, for the track of the selected clip or note: eight square 24px swatches (teal, cyan, blue, violet, magenta, rose, lime, steel) and a Default button that returns the track to phosphor (audio) or amber (notes). The chosen swatch takes a 2px ink outline and a dark tick, and every swatch has a spoken name, so the choice never rides on colour alone. All eight sit at oklch lightness 0.8, which gives 9.6:1 or more against `screen`, and none is near vermilion. The colour is stored in `project.json` as `#rrggbb`, changes through the undo history, and is applied through `--trace` and `--trace-dim` on the track's header and lane: clip border, tint and waveform, the note line, box border and hatch, and the kind icon in the header. Kind still reads from the fill (solid waveform, hatched box), and a muted track is drawn hollow and dashed in the dim version of its colour.

### Track Headers

Headers use a 28px identity row with a fixed mute button and a second row for the audio destination or track kind. The muted tag sits on the second row so the name keeps its width. Voice order blocks draw their stored audio waveform below the dialogue and status, using the same renderer as audio clips and the backend’s exact source trim offset. Audio and operator tracks are 120px tall with 4px gaps and 8px vertical padding; notes, MIDI and OSC remain 64px with 2px gaps and 4px vertical padding. The audio destination is a recessed 24px button with an output icon, truncating name and inspector chevron. Its tooltip and accessible name include the full destination. Keyboard focus uses the neutral inset rule.

### Fader and Level Meter
Both sit in an audio track's header under the name row, which is why an audio lane is 120px tall. The fader is a recessed rail with a 1px notch at 0 dB and a square ink thumb (phosphor on hover), the value in measure type to its right; range -inf to +6 dB, 1 dB per arrow key, 0.1 with Shift, 6 per Page key, double-click returns to 0 dB. The meter is 24 square segments of 2.5 dB on the screen ground, off = `lamp-off`, lit = phosphor up to -12 dBFS, amber to -3, vermilion above. Flat fills only, no glow, no gradient. The peak-hold marker is one lone lit segment above the bar. A printed scale (-48, -24, -12, 0 with ticks between) sits below, and the accessible value says the level in words, so colour never carries the reading alone. The CLIP lamp is printed text that fills vermilion when the peak reached 0 dBFS and stays until pressed. The bar rises at once and falls at 24 dB/s, the marker holds 1.5 s then falls at 12 dB/s; with reduced motion the bar follows the signal and the marker jumps instead of sliding.

### Status Bar and Dialog
30px recessed bar: informational message left, tabular measurements right. Operation errors appear in one native alert dialog above the current task, with a translated heading, explanation and acknowledgement button. Escape dismisses the current error; distinct pending errors follow one at a time and duplicate pending messages are coalesced. The dialog preserves the underlying task and blocks transport shortcuts. The error and unsaved dialogs are square panels, max 420px, with a flat backdrop.

### Preferences Dialog
Everything that is set up rather than played lives in one modal opened from File > Preferences, Ctrl+, (⌘, on macOS) or a faceplate lamp. A native `<dialog>`, 760x520 (92vw / 88vh at most), square, on the dialog's flat backdrop: a 44px recessed title row with a close button, a 200px page list on the left (Timeline, Project, Audio, Metronome, LTC outputs, MTC outputs, OSC destinations, Remote control, Graphics, General, Video), the page on the right, and a recessed footer that prints what the output is doing (running device and rate, "Changes apply when playback stops", and any warnings in amber). It is modal because setup is rare, holds lists that would crowd the 300px inspector, and the transport keys must not fire behind it; the faceplate keeps the live state in view. Output lists are rows between 1px rules, not cards: a name (LTC 1), a lamp with its status word, a remove button, and below them the device or port select and, for LTC, the channel select. Outputs connect by themselves when a choice is made; a change made while playing waits for stop or pause.

### Remote control preferences

The Remote control page has a MIDI block and an OSC block, each with a switch, its lamp and the lamp's word (Listening, Missing, Off, Error). MIDI adds the input port select and a list of assignments between 1px rules: the pad as a controller's manual writes it (`NOTE C2 · CH 1`), an action select and, for the actions that take a number, a monospace number. A new assignment is made by picking an action and pressing Learn, which waits for the next pad (a status line says so, with Cancel beside it). OSC prints its fixed addresses in monospace for the user to copy. The choices are saved at once and belong to the machine.

### Snap

The magnet pulls dragged clips, notes, handles, envelope points and the playhead onto the grid, the edges of other clips and notes, the playhead and frame 0 when one is within 8px; either edge of a dragged block can land. The Snap control in the timeline bar is a lamp and a name (on is phosphor, off is `lamp-off`, and the word is always printed) with a chevron that opens a flat menu of what it pulls onto and the grid step (automatic, frame, second, beat, bar). G toggles it; holding Alt during a drag turns it the other way for that drag. While it holds a drag a 1px ink guide line runs the height of the lanes: solid for a grid line, dashed for a clip, note, playhead or origin, so the kind never rides on colour. The guide is never vermilion, because vermilion means live or fault.

### Graphics preferences

The Graphics page of Preferences holds a hardware-acceleration switch and, on Windows, a graphics API list (Automatic, Direct3D 11, Direct3D 12, OpenGL, Vulkan marked experimental). The webview reads them once when it starts, so a choice is saved at once and applied after a restart, which the page offers (asking first about unsaved work, and not while the output plays). `--safe-graphics` or `FLUX_SAFE_GRAPHICS=1` ignores the saved choice for one run.

## Do's and Don'ts

### Do:
- **Do** keep the product dark-only; scene is booth or stage.
- **Do** use phosphor for reading, amber for cues and hold, vermilion for live and fault only.
- **Do** tell every kind and state apart by a second cue as well as colour (fill pattern, outline, dash, printed word).
- **Do** keep corners square (2px at most) and rules at 1px, 2px for plate edges.
- **Do** keep the dot-matrix cell on whole device pixels.
- **Do** put every user-facing string through i18n (English and Spanish) and test layouts with the longer Spanish text.
- **Do** keep monospace to measurement and tabular numerals on all time values.
- **Do** keep the existing themed selection, caret, neutral focus line, scrollbars and reduced-motion handling on new surfaces.

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
- A clip whose audio has not been read yet shows a placeholder waveform; once its peaks arrive the real one replaces it.
- There is no add-track control; tracks are created by importing audio or pressing M.
- No engraved legend face is shipped; the user accepted the system stack. Reopen only if the product wants a printed-legend face later.
- The seed reference in the contract (`97c645d5`) is the raw key the roll printed, kept as printed.
- Minor build drift: a few component fills repeat token values as literals (key and button hover `oklch(0.35 0.016 250)`, primary hover `oklch(0.92 0.17 128)`, clip tints); run-info warnings use amber outside the cue/hold meaning.

### Project and computer settings

Preferences retain their existing pages and instrument styling. Output pages explicitly label **Project** controls
(signals and levels) and **This computer** controls (devices, channels and latency), with a note that local routing
is shared by projects. Legacy project routes are offered on the Project page through an inline before/after table
and an explicit apply action. Local preference operation errors use the shared error dialog; persistent configuration warnings remain visible in the footer.

General uses a dedicated language listbox with a 20×15 flag and its readable native language name. English uses
the GB flag and Spanish uses ES, bundled from `flag-icons`; flags are decorative and hidden from assistive
technology. The selected control and both options show the flag. The flat menu supports arrows, Home/End,
Enter/Space, Escape and focus return, and closes when focus or a pointer moves outside it. Language changes apply
immediately to every window and persist in the computer preferences.
