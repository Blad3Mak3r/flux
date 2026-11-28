# Editing the timeline

The editor works on clips, notes, MIDI events, OSC events and DMX scenes (the operator's orders are moved one by one).
Everything below is one step of undo, however many items it touches.

## Selecting several items

| Gesture | Does |
| --- | --- |
| Click | Selects one item. |
| Ctrl/Cmd+click | Adds the item to the selection, or takes it out. |
| Shift+click | Selects every item of that track from the last one you selected to this one. |
| Drag on an empty part of the lanes | Selects what the box touches, on any track. Hold Ctrl/Cmd or Shift to add to the selection. |
| Ctrl/Cmd+A | Selects every item of the active timeline. |
| Esc | Clears the selection. |

The Inspector shows "N items selected" while two or more are.

## Moving and deleting

Drag any selected item and the whole selection moves by the same number of frames. Snapping follows the item you hold
(see the Snap control); Alt turns it the other way for that drag. The group stops together at the start of the
timeline, so the items keep their distances. The arrow keys nudge the group by a frame, Shift by 25 frames.
Delete removes the selection.

## Ripple

Ripple editing makes the later items follow an edit. Choose the mode with the Ripple control in the timeline bar
(next to Snap), the Edit menu, or Alt+P, which steps through the modes:

- **Off**: only the selected items change.
- **Per track**: on each track the edit touches, the items after it move by the same amount. Deleting a clip closes
  up the room it took; moving one by some frames shifts what follows it by the same frames.
- **All tracks**: the same, on every track, from the first selected item on.

The mode is remembered on this computer.

## Markers, regions and the tempo map

Under the ruler is a lane for markers, regions and tempo changes. Everything in it is one step of undo and is kept in
order by the backend, which also names new ones (Marker 1, Region 2, ...), refuses a blank name and keeps each tempo change
on a bar.

- **Markers** are named points. **M** (or Transport > Add marker at playhead) adds one at the playhead; drag it to move
  it, double-click it to rename, right-click for its colour and Delete, and press Delete when it is picked.
  **Alt+Left** and **Alt+Right** go to the previous and next marker (Transport menu too). Markers and the edges of regions
  are targets of the magnet (Snap > Markers and regions) and have a dashed line across the tracks.
- **Regions** are named stretches. Drag in the empty lane to draw one, or select some clips or notes and press **R**
  (Transport > Region from selection) to cover them. Drag a region to move it, drag its edges to resize it, double-click to
  rename. Regions are only marked and navigated for now: Flux has no loop playback, so a region does not loop.
- **Tempo changes** start a new tempo and time signature from a bar. The tempo bar holds the first point (BPM, beats per bar
  and the note a beat is); **+ Change** or the lane's context menu adds a change at the bar nearest the playhead (or where
  you right-clicked), with the tempo in force there. Pick a change to edit its BPM and signature or delete it, and drag it to
  another bar. Set the ruler to **Bars** to count bars and beats across the changes; the grid, the snap (Beat and Bar steps
  too) and the metronome follow the same map, also after a seek, and the MIDI clock out follows its tempo.

Markers and regions are shown in the editor only; the Live view does not show them yet.

## Zoom to fit

The button beside the zoom controls, View > Zoom to fit, or Ctrl/Cmd+0 zooms and scrolls so that the selected items
fill the view. With nothing selected it fits the whole active timeline.

## Zoom and long shows

Only the items near the screen are drawn, so a timeline with thousands of notes stays smooth when you scroll and zoom.
An item you are dragging or have selected always stays.
