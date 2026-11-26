// The menu bar and the keys of a VJ show. It has no timeline, transport or tracks: its menus act on
// the grid (composition, decks, layers, clips) and its output, and its keys launch and stop clips.

import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MessageKey, Translate } from "../i18n/messages";
import { errorMessage, pickMediaPaths } from "../project/api";
import type { VjClip, VjDeck, VjGrid, VjLayer, VjSide } from "../project/types";
import { dispatch, useDoc } from "../state/store";
import { openSetup, sayError } from "../state/ui";
import type { MenuItem } from "../state/ui";
import { closeDocument, newDocument, openDocument, saveDocument } from "../state/workflows";
import { hasNativeWindow } from "../components/WindowControls";
import { stop, toggle } from "../transport/playback";
import { MOD_KEY, helpMenu } from "../components/menus";
import { cue, edit, importVideos, launch, launchColumn, moveCrossfader, showBrowser, showTab, stopLayer, useVj } from "./store";

const mod = (keys: string) => `${MOD_KEY}+${keys}`;
const fail = (err: unknown) => sayError(errorMessage(err));

const grid = (): VjGrid | undefined => useDoc.getState().project.vj;
const shownDeck = (vj: VjGrid): VjDeck | undefined => vj.decks.find((deck) => deck.id === vj.deck);

/** The layer the inspector shows, or none. */
function selectedLayer(): VjLayer | undefined {
  const id = useVj.getState().layer;
  return grid()?.layers.find((layer) => layer.id === id);
}

/** The clip the inspector shows and its layer, or none. */
function selectedClip(): { clip: VjClip; layer: VjLayer } | undefined {
  const id = useVj.getState().clip;
  for (const layer of grid()?.layers ?? []) {
    const clip = layer.clips?.find((c) => c.id === id);
    if (clip) return { clip, layer };
  }
}

/** Where imported media lands: the selected layer, or the first deck layer. */
function importTarget(): VjLayer | undefined {
  const vj = grid();
  return selectedLayer() ?? vj?.layers.find((layer) => !layer.global) ?? vj?.layers[0];
}

export async function importMedia(): Promise<void> {
  const layer = importTarget();
  if (!layer) return;
  try {
    await importVideos(await pickMediaPaths(), layer.id, null);
  } catch (err) {
    fail(err);
  }
}

/** Shows the deck `step` tabs away from the one shown, if there is one. */
export function stepDeck(step: number): void {
  const vj = grid();
  if (!vj) return;
  const at = vj.decks.findIndex((deck) => deck.id === vj.deck) + step;
  const deck = vj.decks[at];
  if (deck) void edit({ op: "selectDeck", deck: deck.id });
}

/** Launches the `n`-th column (from 0) of the deck shown. */
export function launchColumnAt(n: number): void {
  const vj = grid();
  const column = vj && shownDeck(vj)?.columns[n];
  if (column) void launchColumn(column.id);
}

export const stopAll = () => void stopLayer(null);

export function launchSelected(): void {
  const found = selectedClip();
  if (found) void launch(found.clip.id, found.layer.id);
}

/** Jumps the selected clip to its `n`-th cue (from 0), launching it there if it is not playing; nothing when the cue is empty. */
export function cueSelected(n: number): void {
  const found = selectedClip();
  if (found?.clip.cues?.[n] != null) void cue(found.clip.id, n);
}

export function removeSelected(): void {
  const found = selectedClip();
  if (!found) return;
  useVj.setState({ clip: null });
  void edit({ op: "removeClip", clip: found.clip.id });
}

function toggleLayer(op: "layerBypass" | "layerSolo"): void {
  const layer = selectedLayer();
  if (!layer) return;
  const on = op === "layerBypass" ? !layer.bypass : !layer.solo;
  void edit({ op, layer: layer.id, on });
}

/** The Advanced Output: the screens, their slices and masks. */
export const openOutput = () => useVj.setState({ output: true });
export const closeOutput = () => useVj.setState({ output: false });

// ---- Menu bar ----

function fileMenu(t: Translate): MenuItem[] {
  return [
    { label: t("vj.menu.newVj"), shortcut: mod("N"), run: () => newDocument(t, "vj") },
    { label: t("vj.menu.newCue"), run: () => newDocument(t, "timeline") },
    { label: t("menu.open"), shortcut: mod("O"), run: openDocument },
    "-",
    { label: t("menu.save"), shortcut: mod("S"), run: () => void saveDocument(false) },
    { label: t("menu.saveAs"), shortcut: mod("⇧+S"), run: () => void saveDocument(true) },
    { label: t("menu.close"), shortcut: mod("W"), run: closeDocument },
    "-",
    { label: t("vj.menu.importMedia"), shortcut: mod("I"), run: () => void importMedia() },
    "-",
    { label: t("menu.setup"), shortcut: mod(","), run: () => openSetup() },
    ...(hasNativeWindow()
      ? ["-" as const, { label: t("menu.exit"), run: () => void getCurrentWindow().close() }]
      : []),
  ];
}

function editMenu(t: Translate): MenuItem[] {
  const doc = useDoc.getState();
  const clip = selectedClip();
  return [
    { label: t("menu.undo"), shortcut: mod("Z"), disabled: doc.past.length === 0, run: () => dispatch({ type: "undo" }) },
    { label: t("menu.redo"), shortcut: mod("⇧+Z"), disabled: doc.future.length === 0, run: () => dispatch({ type: "redo" }) },
    "-",
    { label: t("vj.clip.remove"), shortcut: "Del", disabled: !clip, run: removeSelected },
    { label: t("menu.deselect"), shortcut: "Esc", disabled: !clip, run: () => useVj.setState({ clip: null }) },
  ];
}

/** The timecode runs from here only on the internal clock. */
const internalClock = () => (grid()?.clock ?? "internal") === "internal";

function compositionMenu(t: Translate): MenuItem[] {
  const crossfader = useVj.getState().play.crossfader;
  const external = !internalClock();
  return [
    { label: t("vj.menu.stopAll"), shortcut: mod("."), run: stopAll },
    "-",
    { heading: t("vj.menu.timecode") },
    { label: t("vj.menu.playPause"), shortcut: "Space", disabled: external, run: () => void toggle() },
    { label: t("vj.menu.toStart"), disabled: external, run: () => void stop() },
    "-",
    { heading: t("vj.menu.crossfader") },
    { label: t("vj.menu.crossA"), checked: crossfader === 0, run: () => void moveCrossfader(0) },
    { label: t("vj.menu.crossMiddle"), checked: crossfader === 0.5, run: () => void moveCrossfader(0.5) },
    { label: t("vj.menu.crossB"), checked: crossfader === 1, run: () => void moveCrossfader(1) },
  ];
}

function deckMenu(t: Translate): MenuItem[] {
  const vj = grid();
  const decks = vj?.decks ?? [];
  const at = vj ? decks.findIndex((deck) => deck.id === vj.deck) : -1;
  const deck = decks[at];
  const columns = deck?.columns ?? [];
  return [
    { label: t("vj.deck.add"), run: () => void edit({ op: "addDeck" }) },
    { label: t("vj.menu.addColumn"), run: () => void edit({ op: "addColumn" }) },
    "-",
    { label: t("vj.menu.previousDeck"), shortcut: mod("PgUp"), disabled: at < 1, run: () => stepDeck(-1) },
    { label: t("vj.menu.nextDeck"), shortcut: mod("PgDn"), disabled: at === -1 || at === decks.length - 1, run: () => stepDeck(1) },
    {
      label: t("vj.menu.goToDeck"),
      disabled: decks.length < 2,
      submenu: decks.map((d, i) => ({
        label: d.name ?? t("vj.deck.default", { n: i + 1 }),
        checked: d.id === vj?.deck,
        run: () => void edit({ op: "selectDeck", deck: d.id }),
      })),
    },
    "-",
    {
      label: t("vj.menu.launchColumn"),
      disabled: columns.length === 0,
      submenu: columns.map((column, i) => ({
        label: `${i + 1}  ${column.name ?? ""}`.trim(),
        shortcut: i < 9 ? String(i + 1) : undefined,
        run: () => void launchColumn(column.id),
      })),
    },
    "-",
    { label: t("vj.deck.remove"), disabled: !deck || decks.length === 1, run: () => deck && void edit({ op: "removeDeck", deck: deck.id }) },
  ];
}

function layerMenu(t: Translate): MenuItem[] {
  const vj = grid();
  const layer = selectedLayer();
  const index = layer && vj ? vj.layers.indexOf(layer) : -1;
  const count = vj?.layers.length ?? 0;
  const side = (value: VjSide | null, key: MessageKey): MenuItem => ({
    label: t(key),
    checked: (layer?.crossfade ?? null) === value,
    disabled: !layer,
    run: () => layer && void edit({ op: "layerCrossfade", layer: layer.id, side: value }),
  });
  return [
    { label: t("vj.menu.addLayer"), run: () => void edit({ op: "addLayer", global: false }) },
    { label: t("vj.menu.addGlobalLayer"), run: () => void edit({ op: "addLayer", global: true }) },
    "-",
    { heading: layer ? layer.name ?? t("vj.layer.default", { n: index + 1 }) : t("vj.menu.noLayer") },
    { label: t("vj.menu.bypass"), shortcut: "B", checked: !!layer?.bypass, disabled: !layer, run: () => toggleLayer("layerBypass") },
    { label: t("vj.menu.solo"), shortcut: "S", checked: !!layer?.solo, disabled: !layer, run: () => toggleLayer("layerSolo") },
    { label: t("vj.layer.eject"), shortcut: "X", disabled: !layer, run: () => layer && void stopLayer(layer.id) },
    {
      label: t(layer?.collapsed ? "vj.layer.expand" : "vj.layer.collapse"),
      disabled: !layer,
      run: () => layer && void edit({ op: "layerCollapse", layer: layer.id, on: !layer.collapsed }),
    },
    {
      label: t("vj.layer.side"),
      disabled: !layer,
      submenu: [side("a", "vj.menu.sideA"), side(null, "vj.menu.sideNone"), side("b", "vj.menu.sideB")],
    },
    "-",
    // Up is towards the top of the drawing, the end of the list.
    { label: t("vj.layer.up"), disabled: !layer || index === count - 1, run: () => layer && void edit({ op: "moveLayer", layer: layer.id, to: index + 1 }) },
    { label: t("vj.layer.down"), disabled: !layer || index < 1, run: () => layer && void edit({ op: "moveLayer", layer: layer.id, to: index - 1 }) },
    { label: t("vj.layer.remove"), disabled: !layer || count === 1, run: () => layer && void edit({ op: "removeLayer", layer: layer.id }) },
  ];
}

function clipMenu(t: Translate): MenuItem[] {
  const found = selectedClip();
  return [
    { label: t("vj.menu.launchClip"), shortcut: "Enter", disabled: !found, run: launchSelected },
    { label: t("vj.clip.remove"), shortcut: "Del", disabled: !found, run: removeSelected },
  ];
}

function outputMenu(t: Translate): MenuItem[] {
  return [
    { label: t("vj.menu.advancedOutput"), shortcut: mod("⇧+A"), run: openOutput },
  ];
}

function viewMenu(t: Translate): MenuItem[] {
  const { tab, browser } = useVj.getState();
  return [
    { heading: t("vj.menu.inspector") },
    { label: t("vj.inspector.clip"), checked: tab === "clip", run: () => showTab("clip") },
    { label: t("vj.inspector.layer"), checked: tab === "layer", run: () => showTab("layer") },
    "-",
    { heading: t("vj.menu.browser") },
    { label: t("vj.media"), checked: browser === "media", run: () => showBrowser("media") },
    { label: t("vj.browser.generators"), checked: browser === "generators", run: () => showBrowser("generators") },
  ];
}

export const VJ_MENUS: { id: string; label: MessageKey; items: (t: Translate) => MenuItem[] }[] = [
  { id: "file", label: "menu.file", items: fileMenu },
  { id: "edit", label: "menu.edit", items: editMenu },
  { id: "composition", label: "vj.menu.composition", items: compositionMenu },
  { id: "deck", label: "vj.menu.deck", items: deckMenu },
  { id: "layer", label: "vj.menu.layer", items: layerMenu },
  { id: "clip", label: "vj.menu.clip", items: clipMenu },
  { id: "output", label: "vj.menu.output", items: outputMenu },
  { id: "view", label: "menu.view", items: viewMenu },
  { id: "help", label: "menu.help", items: helpMenu },
];

// ---- Keys ----

/**
 * The keys of a VJ show, called with no dialog open and nothing being typed in; `true` when it used
 * the key. The timeline's keys (transport, notes, split, snap) mean nothing here.
 */
export function vjKeyDown(event: KeyboardEvent, t: Translate): boolean {
  const mod = event.ctrlKey || event.metaKey;
  const key = event.key.toLowerCase();
  const onButton = event.target instanceof Element && event.target.closest("button, select, [role='button']");
  let run: (() => void) | null = null;
  if (mod) {
    run =
      key === "z" ? () => dispatch({ type: event.shiftKey ? "redo" : "undo" })
      : key === "y" ? () => dispatch({ type: "redo" })
      : key === "s" ? () => void saveDocument(event.shiftKey)
      : key === "o" ? openDocument
      : key === "n" ? () => newDocument(t, "vj")
      : key === "w" ? closeDocument
      : key === "," ? () => openSetup()
      : key === "i" ? () => void importMedia()
      : key === "." ? stopAll
      : key === "a" && event.shiftKey ? openOutput
      : key === "pageup" ? () => stepDeck(-1)
      : key === "pagedown" ? () => stepDeck(1)
      : null;
  } else if (event.shiftKey && !event.altKey && /^Digit[1-8]$/.test(event.code)) {
    // Shift and 1 to 8 jump to the cues of the selected clip, as the digits alone launch columns.
    run = () => cueSelected(Number(event.code.slice(5)) - 1);
  } else if (event.key === " " && !onButton) {
    run = () => {
      if (internalClock()) void toggle();
    };
  } else if (/^[1-9]$/.test(event.key)) {
    run = () => launchColumnAt(Number(event.key) - 1);
  } else if (event.key === "Enter" && !onButton) {
    run = launchSelected;
  } else if (event.key === "Delete" || event.key === "Backspace") {
    run = removeSelected;
  } else if (event.key === "Escape") {
    run = () => useVj.setState({ clip: null });
  } else if (key === "b") {
    run = () => toggleLayer("layerBypass");
  } else if (key === "s") {
    run = () => toggleLayer("layerSolo");
  } else if (key === "x") {
    run = () => {
      const layer = selectedLayer();
      if (layer) void stopLayer(layer.id);
    };
  }
  if (!run) return false;
  event.preventDefault();
  run();
  return true;
}
