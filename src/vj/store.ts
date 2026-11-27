// What the VJ screen shows besides the project: what the grid plays, which clip is selected and a
// fader being moved. The backend owns the grid and what plays; this only keeps its last answer.

import { create } from "zustand";
import { errorMessage } from "../project/api";
import { dispatch, useDoc } from "../state/store";
import { say, sayError } from "../state/ui";
import { probeVideos } from "../state/video";
import {
  vjBeat,
  vjCrossfader,
  vjCrossfaderReset,
  vjCue,
  vjEdit,
  vjLaunch,
  vjLaunchColumn,
  vjLaunchGroupColumn,
  vjLayout,
  vjMonitorFrame,
  vjOutputFrame,
  vjScreenFrame,
  vjPreviewFrame,
  vjPreview,
  vjPlayState,
  vjRelease,
  vjScrub,
  vjImport,
  vjModulationValues,
  vjStop,
} from "./api";
import type { DockNode, DockTarget, GridState, LayoutState, ModValues, MonitorKind, VjBeatOp, VjOp } from "./api";

/** A video file being copied into the project, shown where it will land until it is there. */
export interface Importing {
  key: number;
  /** The file's name. */
  name: string;
  layer: string;
  /** The cell's column, or `null` for the layer's first empty cell. */
  column: string | null;
}

interface VjState {
  play: GridState;
  importing: Importing[];
  /** The clip the inspector shows. */
  clip: string | null;
  /** The empty slot selected instead of a clip (`layer/column`): the monitor and the inspector show nothing. */
  slot: string | null;
  /** The layer the inspector shows. */
  layer: string | null;
  /** The group the inspector shows. */
  group: string | null;
  /** The layers picked with Ctrl or Cmd held, which Group selected layers puts in a group; only the screen's. */
  picked: string[];
  /** What was selected last, which the browser and the group rows follow; its panel is brought forward. */
  tab: "clip" | "layer" | "group" | "composition" | "mappings";
  /** The dock layout of the screen and the names it can switch to; `null` until the backend answers. */
  layout: LayoutState | null;
  /** Which media the browser lists. */
  media: MediaKind;
  /** The effects of the inspector folded shut; only the screen's, never the file's. */
  folded: Record<string, true>;
  /** The modulation sub-rows of the inspector that are open, by the setting's key; only the screen's. */
  modOpen: Record<string, true>;
  /** The modulated values now, while the inspector shows something modulated. */
  mod: ModValues;
  /** Whether the Advanced Output is open. */
  output: boolean;
}

const NO_MOD: ModValues = { effects: {}, layers: {}, clips: {}, knobs: {} };

export type MediaKind = "all" | "video" | "image";

export const useVj = create<VjState>()(() => ({
  play: { playing: [], column: null, crossfader: 0.5, beats: 0, beatsPaused: false },
  importing: [],
  clip: null,
  slot: null,
  layer: null,
  group: null,
  picked: [],
  tab: "clip",
  media: "all",
  layout: null,
  folded: {},
  modOpen: {},
  mod: NO_MOD,
  output: false,
}));

const fail = (err: unknown) => sayError(errorMessage(err));

/** Applies an edit of the grid in the backend; the answer enters the undo history. `false` when the backend refused it. */
export async function edit(op: VjOp): Promise<boolean> {
  // A move still on its way must not land after the value kept.
  await moving;
  try {
    const { project, active } = await vjEdit(useDoc.getState().project, op);
    dispatch({ type: "apply", project, active });
    return true;
  } catch (err) {
    fail(err);
    return false;
  }
}

let imports = 0;
const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/**
 * Adds video files to `layer`, the first in the cell of `column` (or each in the layer's first empty
 * cell with `null`), then reads them: the clips show their state meanwhile.
 */
export async function importVideos(paths: string[], layer: string, column: string | null): Promise<void> {
  for (const [at, path] of paths.entries()) {
    const key = ++imports;
    const name = fileName(path);
    const cell = at === 0 ? column : null;
    useVj.setState((vj) => ({ importing: [...vj.importing, { key, name, layer, column: cell }] }));
    try {
      const before = useDoc.getState().project.videos ?? [];
      const { project, active } = await vjImport(useDoc.getState().project, path, layer, cell);
      dispatch({ type: "apply", project, active });
      const added = (project.videos ?? []).filter((video) => !before.some((old) => old.id === video.id));
      say("status.imported", { name });
      void probeVideos(added);
    } catch (err) {
      fail(err);
    } finally {
      useVj.setState((vj) => ({ importing: vj.importing.filter((item) => item.key !== key) }));
    }
  }
}

/**
 * Sends a level while its fader moves, so the output follows the hand: the backend takes it at
 * once, and only the last one, sent with {@link edit} when the fader is let go, enters the history.
 * One goes at a time; moves made meanwhile are folded into the newest.
 */
let next: VjOp | null = null;
let moving: Promise<void> = Promise.resolve();
export function preview(op: VjOp): void {
  const idle = next === null;
  next = op;
  if (!idle) return;
  moving = moving.then(async () => {
    const op = next;
    next = null;
    if (op) await vjPreview(op).catch(() => undefined);
  });
}

const played = (run: () => Promise<GridState>) =>
  run()
    .then((play) => useVj.setState({ play }))
    .catch(fail);

export const launch = (clip: string, layer: string) => {
  useVj.setState({ clip, slot: null, layer, tab: "clip" });
  reveal("clip");
  return played(() => vjLaunch(clip));
};
export const release = (clip: string) => played(() => vjRelease(clip));
export const launchColumn = (column: string) => played(() => vjLaunchColumn(column));
export const launchGroupColumn = (group: string, column: string) => played(() => vjLaunchGroupColumn(group, column));
export const stopLayer = (layer: string | null) => played(() => vjStop(layer));
export const beatClock = (op: VjBeatOp) => played(() => vjBeat(op));
export const cue = (clip: string, index: number) => played(() => vjCue(clip, index));

/**
 * Moves the playhead of the clip on `layer` while the hand scrubs: one goes at a time, and moves made meanwhile are
 * folded into the newest, as {@link preview} does.
 */
let nextScrub: { layer: string; position: number } | null = null;
export function scrub(layer: string, position: number): void {
  const idle = nextScrub === null;
  nextScrub = { layer, position };
  if (!idle) return;
  scrubbing = scrubbing.then(async () => {
    const to = nextScrub;
    nextScrub = null;
    if (to) await vjScrub(to.layer, to.position).then((play) => useVj.setState({ play })).catch(() => undefined);
  });
}
let scrubbing: Promise<void> = Promise.resolve();
export const moveCrossfader = (position: number) => played(() => vjCrossfader(position));
export const resetCrossfader = () => played(() => vjCrossfaderReset());

export const selectClip = (clip: string, layer: string) => {
  useVj.setState({ clip, slot: null, layer, tab: "clip" });
  reveal("clip");
};
/** Selects an empty slot: no clip is selected, so the preview and the clip panel show their empty state. */
export const selectSlot = (layer: string, column: string) => {
  useVj.setState({ clip: null, slot: `${layer}/${column}`, layer, tab: "clip" });
  reveal("clip");
};
/** Shows the layer in the inspector; with `pick` it also joins (or leaves) the layers picked for a group. */
export const selectLayer = (layer: string, pick = false) => {
  useVj.setState((vj) => ({
    layer,
    tab: "layer",
    picked: pick ? (vj.picked.includes(layer) ? vj.picked.filter((id) => id !== layer) : [...vj.picked, layer]) : [],
  }));
  reveal("layer");
};
export const selectGroup = (group: string) => {
  useVj.setState({ group, tab: "group", picked: [] });
  reveal("group");
};
/** Brings a panel forward, opening it when it is closed (the View menu and the Composition key). */
export const showTab = (tab: VjState["tab"]) => {
  useVj.setState({ tab });
  reveal(tab, true);
};

// ---- The dock layout ----

/** Where the panel `panel` is in the tree: its tab group and its place there. */
function seat(node: DockNode, panel: string): { group: string; index: number; active: boolean } | null {
  if (node.type === "tabs") {
    const index = node.panels.indexOf(panel);
    return index < 0 ? null : { group: node.id, index, active: node.active === index };
  }
  if (node.type === "split") return node.children.reduce<ReturnType<typeof seat>>((found, child) => found ?? seat(child, panel), null);
  return null;
}

/** The ids of the panels that are open, in the tree's order. */
export function openPanels(node: DockNode): string[] {
  if (node.type === "tabs") return node.panels;
  return node.type === "split" ? node.children.flatMap(openPanels) : [];
}

const laid = (run: () => Promise<LayoutState>) =>
  run()
    .then((layout) => useVj.setState({ layout }))
    .catch(fail);

export const loadLayout = () => laid(() => vjLayout("get"));
export const layoutMove = (panel: string, target: DockTarget) => laid(() => vjLayout("move", { panel, target }));
export const layoutReorder = (group: string, from: number, to: number) => laid(() => vjLayout("reorder", { group, from, to }));
export const layoutActivate = (group: string, index: number) => laid(() => vjLayout("activate", { group, index }));
export const layoutClose = (panel: string) => laid(() => vjLayout("close", { panel }));
export const layoutOpen = (panel: string, monitor?: MonitorKind) => laid(() => vjLayout("open", { panel, monitor }));
export const layoutResize = (split: string, sizes: number[]) => laid(() => vjLayout("resize", { split, sizes }));
export const layoutEqualize = (split: string) => laid(() => vjLayout("equalize", { split }));
export const layoutSetMonitor = (id: string, kind: MonitorKind) => laid(() => vjLayout("set_monitor", { id, kind }));
export const layoutApply = (id: string) => laid(() => vjLayout("apply", { id }));
export const layoutSave = (name: string) => laid(() => vjLayout("save", { name }));
export const layoutDelete = (id: string) => laid(() => vjLayout("delete", { id }));
export const layoutReset = () => laid(() => vjLayout("reset"));

/** Brings the tab of `panel` forward when it is open and hidden behind another; a closed one opens only with `open`. */
function reveal(panel: string, open = false): void {
  const layout = useVj.getState().layout;
  if (!layout) return;
  const at = seat(layout.current.root, panel);
  if (at && !at.active) void layoutActivate(at.group, at.index);
  else if (!at && open) void layoutOpen(panel);
}
export const showMedia = (media: MediaKind) => useVj.setState({ media });
/** Folds an effect of the inspector shut, or opens it again. */
export const foldEffect = (effect: string) =>
  useVj.setState((vj) => {
    const folded = { ...vj.folded };
    if (folded[effect]) delete folded[effect];
    else folded[effect] = true;
    return { folded };
  });
/** Opens the modulation sub-row of a setting (`open`), closes it, or flips it with no `open`. */
export const showModulation = (key: string, open?: boolean) =>
  useVj.setState((vj) => {
    const modOpen = { ...vj.modOpen };
    if (open ?? !modOpen[key]) modOpen[key] = true;
    else delete modOpen[key];
    return { modOpen };
  });

/** Asks the backend what plays, twenty times a second, while the VJ screen is open and in sight. */
export function bindPlayState(): () => void {
  let stopped = false;
  const tick = () => {
    if (stopped) return;
    // A minimized window shows nothing: the screens play on without it.
    if (document.hidden) {
      window.setTimeout(tick, 100);
      return;
    }
    vjPlayState()
      .then((play) => {
        if (!stopped) useVj.setState({ play });
      })
      .catch(() => undefined)
      .finally(() => {
        if (!stopped) window.setTimeout(tick, PLAY_EVERY);
      });
  };
  tick();
  return () => {
    stopped = true;
  };
}

/** How often the play state is asked for: the playhead of a clip moves with it. */
const PLAY_EVERY = 50;

/** How often the modulated values are asked for while the inspector shows something modulated: thirty times a second. */
const MOD_EVERY = 1000 / 30;

/** Asks the backend for the modulated values {@link MOD_EVERY}, to mark them on the controls. The UI never computes them. */
export function bindModulation(): () => void {
  let stopped = false;
  const tick = () => {
    if (stopped) return;
    if (document.hidden) {
      window.setTimeout(tick, 100);
      return;
    }
    vjModulationValues()
      .then((mod) => {
        if (!stopped) useVj.setState({ mod });
      })
      .catch(() => undefined)
      .finally(() => {
        if (!stopped) window.setTimeout(tick, MOD_EVERY);
      });
  };
  tick();
  return () => {
    stopped = true;
    useVj.setState({ mod: NO_MOD });
  };
}

/** The backend's picture: its width and height as two little-endian u16, then its RGBA pixels. */
function picture(bytes: ArrayBuffer): ImageData | null {
  if (bytes.byteLength < 4) return null;
  const size = new DataView(bytes);
  const width = size.getUint16(0, true);
  const height = size.getUint16(2, true);
  return new ImageData(new Uint8ClampedArray(bytes, 4, width * height * 4), width, height);
}

/** How often the output monitor asks for a picture: thirty times a second. */
const OUTPUT_EVERY = 1000 / 30;

/**
 * Asks the backend for the picture the screens show {@link OUTPUT_EVERY}, with up to two asks on the
 * way so one's trip does not hold the next, and hands the newest to `draw` (`null` when there is
 * none), while the VJ screen is open and in sight.
 */
export function bindOutput(draw: (picture: ImageData | null) => void): () => void {
  return bindFrames(vjOutputFrame, draw);
}

/** Like {@link bindOutput}, for what one screen outputs: the composition through its slices and masks. */
export function bindScreen(screen: string, draw: (picture: ImageData | null) => void): () => void {
  return bindFrames(() => vjScreenFrame(screen), draw);
}

/** Like {@link bindOutput}, for what one layer or group alone contributes to the composition. */
export function bindMonitor(kind: "layer" | "group", id: string, draw: (picture: ImageData | null) => void): () => void {
  return bindFrames(() => vjMonitorFrame(kind, id), draw);
}

/** Like {@link bindOutput}, for the picture of one clip alone as the composition would show it. */
export function bindPreview(clip: string, draw: (picture: ImageData | null) => void): () => void {
  return bindFrames(() => vjPreviewFrame(clip), draw);
}

function bindFrames(frame: () => Promise<ArrayBuffer>, draw: (picture: ImageData | null) => void): () => void {
  let stopped = false;
  let pending = 0;
  let asked = 0;
  let drawn = 0;
  const ask = () => {
    if (stopped || pending >= 2 || document.hidden) return;
    pending += 1;
    const number = ++asked;
    frame()
      .then((bytes) => {
        if (stopped || number < drawn) return;
        drawn = number;
        draw(picture(bytes));
      })
      .catch(() => undefined)
      .finally(() => {
        pending -= 1;
      });
  };
  const timer = window.setInterval(ask, OUTPUT_EVERY);
  return () => {
    stopped = true;
    window.clearInterval(timer);
  };
}
