import { Fragment, useEffect, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, MouseEvent, PointerEvent, ReactNode } from "react";
import { InlineName } from "../components/InlineName";
import { Marquee } from "../components/Marquee";
import { ChevronDownIcon, ChevronRightIcon, CloseIcon, GlobeIcon, ImportIcon, PlayIcon, PlusIcon, StopIcon } from "../components/icons";
import { useT } from "../i18n";
import { errorMessage, pickMediaPaths } from "../project/api";
import { MediaFile } from "../media/MediaFile";
import { MediaProgressBar } from "../media/progress";
import { focusMedia, useMedia } from "../media/store";
import { useVideoErrors } from "../state/video";
import type {
  VideoAsset,
  VideoFit,
  VjBlend,
  VjClip,
  VjGenerator,
  VjGeneratorKind,
  VjGroup,
  VjColumn,
  VjDeck,
  VjEffect,
  VjGrid,
  VjLayer,
  VjPlayback,
  VjSide,
  VjTarget,
  VjAutopilot,
  VjTrigger,
} from "../project/types";
import { useDoc } from "../state/store";
import { startDrag, useDropTarget } from "../state/pointerDrag";
import { useFileDrop } from "../state/fileDrop";
import { openMenu, sayError } from "../state/ui";
import type { MenuItem } from "../state/ui";
import type { VideoStrip } from "../video/api";
import {
  bindOutput,
  bindPreview,
  bindPlayState,
  edit,
  importVideos,
  bindModulation,
  launch,
  launchColumn,
  launchGroupColumn,
  moveCrossfader,
  preview,
  release,
  selectClip,
  selectGroup,
  selectLayer,
  showBrowser,
  showMedia,
  showTab,
  stopLayer,
  useVj,
} from "./store";
import type { MediaKind } from "./store";
import { GENERATORS, swatch } from "./generators";
import { clipName, groupName, layerName } from "./names";
import { groupItems } from "./menus";
import { Dashboard } from "./Dashboard";
import { MapBar, MapLayer } from "./MapLayer";
import { MapImportDialog } from "./MapImportDialog";
import { LibraryDialog, NameDialog } from "./PresetDialogs";
import { loadPresets } from "./presets";
import { bindVjControl, useMap } from "./mapping";
import { mappable } from "./mapModel";
import { MappingsPanel } from "./Mappings";
import { stepDeck } from "./menus";
import { bindOutputState, toggleDisable, useOutput } from "./output";
import { AdvancedOutput } from "./AdvancedOutput";
import { ColourField, Scrub, Segments } from "./controls";
import { CompositionFx, EffectBadge, EffectBrowser, EffectStack, carriesEffect, dropEffect, useCompositionDrop } from "./EffectStack";
import type { EffectTarget } from "./EffectStack";
import { loadEffectCatalog, useEffectCatalog } from "./effects";
import { AnalysisMeter, Tempo, TimecodeClock } from "./CompositionBar";
import { GeneratorModPanel, GeneratorScrub, ModPanel, ModScrub, fieldRange, inspectorModulated } from "./Modulation";
import { TextFields } from "./TextFields";
import { Transport } from "./Transport";
import "./vj.css";

/** One empty list for a project without videos, so a store selector answers the same value each time. */
const NO_VIDEOS: VideoAsset[] = [];
const NO_EFFECTS: VjEffect[] = [];

/** Whether a video can be shown yet: `offline` when its file is gone, `failed` when the backend cannot read it, `processing` while it reads it. */
function useVideoState(asset: string | undefined): "offline" | "failed" | "processing" | null {
  const error = useVideoErrors((errors) => (asset ? errors[asset] : undefined));
  const offline = useMedia((media) => asset !== undefined && media.offline.has(asset));
  const ready = useDoc((doc) => !asset || (asset in doc.video && asset in doc.thumbs));
  if (offline) return "offline";
  if (error) return "failed";
  return ready ? null : "processing";
}

/** The state of a clip's video over its picture: importing, processing or unreadable. */
function VideoState({ state, asset }: { state: "importing" | "processing" | "failed" | "offline"; asset?: string }) {
  const t = useT();
  const error = useVideoErrors((errors) => (asset === undefined ? undefined : errors[asset]));
  return (
    <span className={`vj-state ${state}`} title={error}>
      {state !== "failed" && state !== "offline" && <i className="vj-spin" aria-hidden="true" />}
      {t(`vj.state.${state}`)}
    </span>
  );
}

// Grouped as Resolume lists them.
const BLEND_GROUPS: { group: "general" | "darken" | "lighten" | "contrast" | "comparative" | "colour" | "keys"; blends: VjBlend[] }[] = [
  { group: "general", blends: ["alpha"] },
  { group: "darken", blends: ["darken", "multiply", "colorBurn"] },
  { group: "lighten", blends: ["lighten", "screen", "colorDodge"] },
  { group: "contrast", blends: ["overlay", "softLight", "hardLight", "linearLight", "vividLight", "pinLight"] },
  { group: "comparative", blends: ["add", "subtract", "difference", "differenceInverted", "exclusion"] },
  { group: "colour", blends: ["hue", "saturation", "color", "luminosity"] },
  { group: "keys", blends: ["alphaCut", "lumaKey", "lumaKeyInverted", "lumaIsAlpha", "mask50", "rgb", "displace"] },
];

function BlendOptions() {
  const t = useT();
  return (
    <>
      {BLEND_GROUPS.map(({ group, blends }) => (
        <optgroup key={group} label={t(`vj.blend.group.${group}`)}>
          {blends.map((blend) => <option key={blend} value={blend}>{t(`vj.blend.${blend}`)}</option>)}
        </optgroup>
      ))}
    </>
  );
}
const AUTOPILOTS: VjAutopilot[] = ["off", "next", "previous", "random"];
const PLAYBACKS: VjPlayback[] = ["loop", "once", "pingPong"];
const MEDIA_KINDS: MediaKind[] = ["all", "video", "image"];
const TRIGGERS: VjTrigger[] = ["normal", "piano"];
const FITS: VideoFit[] = ["fit", "fill", "stretch", "none"];
/** The longest transition, in seconds, as the backend keeps it. */
const MAX_TRANSITION = 10;
const DRAG = "application/x-flux-vj";
/** What a layer dragged by its name carries: its id, to drop in or out of a group or at another place. */
const DRAG_LAYER = "application/x-flux-vj-layer";


type Dragged = { clip: string } | { asset: string } | { generator: VjGeneratorKind };

const fail = (err: unknown) => sayError(errorMessage(err));

/** `mm:ss` of a count of seconds. */
function clock(seconds: number): string {
  const whole = Math.max(0, Math.ceil(seconds));
  return `${String(Math.floor(whole / 60)).padStart(2, "0")}:${String(whole % 60).padStart(2, "0")}`;
}

/** The thumbnails of a clip's video; a generator has none. */
const stripOf = (thumbs: Record<string, VideoStrip>, clip: VjClip | undefined) =>
  clip?.asset ? thumbs[clip.asset] : undefined;

/** The clip of `layer` in the cell under `column`, the `at`-th of the deck shown. */
function clipAt(layer: VjLayer, column: VjColumn, at: number): VjClip | undefined {
  return layer.clips?.find((clip) => (layer.global ? clip.slot === at : clip.column === column.id));
}

/** One picture of a video's thumbnail strip, at `position` (0 to 1) of the video, or what a generator draws. */
function Thumb({ strip, generator, position = 0.3, children, className }: {
  strip: VideoStrip | undefined;
  generator?: VjGenerator;
  position?: number;
  children?: ReactNode;
  className?: string;
}) {
  const style: CSSProperties = generator ? swatch(generator) : {};
  if (strip && !generator) {
    const index = Math.min(strip.count - 1, Math.max(0, Math.floor(position * strip.count)));
    style.backgroundImage = `url(${strip.url})`;
    style.backgroundSize = `${strip.count * 100}% 100%`;
    style.backgroundPosition = strip.count > 1 ? `${(index / (strip.count - 1)) * 100}% 0` : "0 0";
  }
  return <div className={`vj-thumb ${className ?? ""}`} style={style}>{children}</div>;
}

/** A vertical fader: its letter is the handle, its colour says what it moves. */
function Fader({ letter, label, value, off, progress, modulated, map, onMove, onSet }: {
  letter: string;
  label: string;
  value: number;
  off?: boolean;
  /** The layer and field of this fader when it is modulated: a mark shows where the modulated value is. */
  modulated?: { layer: string; field: string };
  /** How far what the fader times is, 0 to 1, while it runs: the transition of the T fader. */
  progress?: number | null;
  /** What the fader is in the map mode. */
  map?: VjTarget;
  onMove: (value: number) => void;
  onSet: (value: number) => void;
}) {
  const [held, setHeld] = useState<number | null>(null);
  const shown = held ?? value;
  const now = useVj((vj) => (modulated ? vj.mod.layers[modulated.layer]?.[modulated.field] : undefined));
  const t = useT();
  const at = (event: PointerEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return Math.min(1, Math.max(0, 1 - (event.clientY - box.top) / box.height));
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.key === "ArrowUp" ? 0.05 : event.key === "ArrowDown" ? -0.05 : 0;
    if (!step) return;
    event.preventDefault();
    event.stopPropagation();
    onSet(Math.min(1, Math.max(0, value + step)));
  };
  return (
    <div
      className={`vj-fader${off ? " off" : ""}${modulated ? " modulated" : ""}`}
      data-letter={letter}
      {...mappable(map)}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(shown * 100)}
      title={`${label} ${Math.round(shown * 100)}${modulated ? ` · ${t("vj.layer.modulated")}` : ""}`}
      style={{ "--v": shown, ...(now === undefined ? {} : { "--mv": Math.min(1, Math.max(0, now)) }) } as CSSProperties}
      onKeyDown={onKeyDown}
      onPointerDown={(event) => {
        event.currentTarget.setPointerCapture(event.pointerId);
        const v = at(event);
        setHeld(v);
        onMove(v);
      }}
      onPointerMove={(event) => {
        if (held === null) return;
        const v = at(event);
        setHeld(v);
        onMove(v);
      }}
      onPointerUp={(event) => {
        if (held === null) return;
        const v = at(event);
        setHeld(null);
        onSet(v);
      }}
      onPointerCancel={() => setHeld(null)}
    >
      <i />
      {progress != null && <u style={{ "--p": progress } as CSSProperties} />}
      {now !== undefined && <em aria-hidden="true" />}
      <b>{letter}</b>
    </div>
  );
}

/** The countdown and progress of what `layer` plays (its `clip` only, when given): the one part
 * that changes on every poll, so the cell or header around it does not draw again. */
function PlayTime({ layer, clip, countdown }: { layer: string; clip?: string; countdown: "header" | "cell" }) {
  const shown = (vj: ReturnType<typeof useVj.getState>) =>
    vj.play.playing.find((p) => p.layer === layer && (clip === undefined || p.clip === clip));
  const progress = useVj((vj) => shown(vj)?.progress);
  const remaining = useVj((vj) => shown(vj)?.remaining);
  const left = remaining == null ? null : clock(remaining);
  return (
    <>
      {countdown === "header" && left !== null && <span className="vj-time">-{left}</span>}
      {countdown === "cell" && <span className="vj-air"><PlayIcon />{left ?? ""}</span>}
      {progress != null && <span className="vj-bar"><i style={{ width: `${progress * 100}%` }} /></span>}
    </>
  );
}

function LayerHeader({ grid, layer }: { grid: VjGrid; layer: VjLayer }) {
  const t = useT();
  const playingClip = useVj((vj) => vj.play.playing.find((p) => p.layer === layer.id)?.clip);
  const transition = useVj((vj) => vj.play.playing.find((p) => p.layer === layer.id)?.transition);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const thumbs = useDoc((doc) => doc.thumbs);
  const [renaming, setRenaming] = useState(false);
  const [over, setOver] = useState(false);
  const picked = useVj((vj) => vj.picked.includes(layer.id));
  const name = layerName(t, grid, layer);
  const clip = playingClip === undefined ? undefined : layer.clips?.find((c) => c.id === playingClip);
  const level = (op: "layerMaster" | "layerOpacity" | "layerVolume" | "layerTransition", scale = 1) => ({
    onMove: (value: number) => preview({ op, layer: layer.id, value: value * scale }),
    onSet: (value: number) => void edit({ op, layer: layer.id, value: value * scale }),
  });
  const side = (s: VjSide) => void edit({ op: "layerCrossfade", layer: layer.id, side: layer.crossfade === s ? null : s });
  const fold = () => void edit({ op: "layerCollapse", layer: layer.id, on: !layer.collapsed });
  const at = grid.layers.indexOf(layer);
  const menu: MenuItem[] = [
    { label: t("vj.layer.rename"), run: () => setRenaming(true) },
    { label: t(layer.collapsed ? "vj.layer.expand" : "vj.layer.collapse"), run: fold },
    { label: t("vj.layer.up"), disabled: at === grid.layers.length - 1, run: () => void edit({ op: "moveLayer", layer: layer.id, to: at + 1 }) },
    { label: t("vj.layer.down"), disabled: at === 0, run: () => void edit({ op: "moveLayer", layer: layer.id, to: at - 1 }) },
    "-",
    { heading: t("vj.layer.autopilot") },
    ...AUTOPILOTS.map((mode) => ({
      label: t(`vj.autopilot.${mode}`), checked: (layer.autopilot ?? "off") === mode,
      run: () => void edit({ op: "layerAutopilot", layer: layer.id, mode }),
    })),
    "-",
    { label: t("vj.layer.remove"), disabled: grid.layers.length === 1, run: () => void edit({ op: "removeLayer", layer: layer.id }) },
  ];
  const bs = (
    <div className="vj-bs">
      <button type="button" className={layer.bypass ? "on bypass" : ""} {...mappable({ kind: "layerBypass", layer: layer.id })} aria-pressed={!!layer.bypass} title={t("vj.layer.bypass")}
        onClick={() => void edit({ op: "layerBypass", layer: layer.id, on: !layer.bypass })}>B</button>
      <button type="button" className={layer.solo ? "on solo" : ""} {...mappable({ kind: "layerSolo", layer: layer.id })} aria-pressed={!!layer.solo} title={t("vj.layer.solo")}
        onClick={() => void edit({ op: "layerSolo", layer: layer.id, on: !layer.solo })}>S</button>
    </div>
  );
  const nameRow = (
      <div className="vj-lname" onPointerDown={(event) => startDrag(event, { [DRAG_LAYER]: layer.id }, name)}>
        <button type="button" className="vj-fold" aria-expanded={!layer.collapsed}
          title={t(layer.collapsed ? "vj.layer.expand" : "vj.layer.collapse")} aria-label={t(layer.collapsed ? "vj.layer.expand" : "vj.layer.collapse")}
          onClick={(event) => { event.stopPropagation(); fold(); }}>
          {layer.collapsed ? <ChevronRightIcon /> : <ChevronDownIcon />}
        </button>
        <span className="vj-num">{at + 1}</span>
        {renaming ? (
          <InlineName value={layer.name ?? ""} label={t("vj.layer.rename")} onDone={() => setRenaming(false)}
            onCommit={(text) => void edit({ op: "renameLayer", layer: layer.id, name: text })} />
        ) : (
          <Marquee className="vj-nm" title={name} onDoubleClick={() => setRenaming(true)}>{name}</Marquee>
        )}
        {layer.global && <span className="vj-global" title={t("vj.layer.global")}><GlobeIcon /></span>}
        <EffectBadge count={layer.effects?.length ?? 0} />
        {layer.autopilot && layer.autopilot !== "off" && (
          <span className="vj-ap" title={`${t("vj.layer.autopilot")}: ${t(`vj.autopilot.${layer.autopilot}`)}`}>
            AP {t(`vj.autopilot.short.${layer.autopilot}`)}
          </span>
        )}
        <Marquee className={`vj-clipname${clip ? "" : " idle"}`}>{clip ? clipName(t, clip, videos) : t("vj.layer.noClip")}</Marquee>
        <span className="vj-ab" role="group" aria-label={t("vj.layer.side")}>
          {(["a", "b"] as VjSide[]).map((s) => (
            <button key={s} type="button" className={layer.crossfade === s ? "on" : ""} aria-pressed={layer.crossfade === s}
              onClick={(event) => { event.stopPropagation(); side(s); }}>{s.toUpperCase()}</button>
          ))}
        </span>
      </div>
  );
  const className = `vj-lh${playingClip !== undefined ? " playing" : ""}${layer.global ? " global" : ""}${over ? " over" : ""}${picked ? " picked" : ""}`;
  // An effect dropped on the header goes to the layer's stack; a layer dropped on it takes its place and its group.
  const dropping = useDropTarget({
    accepts: (data) => DRAG_LAYER in data || carriesEffect(data, { layer: layer.id }),
    over: setOver,
    drop: (data) => {
      const dragged = data[DRAG_LAYER];
      if (!dragged) dropEffect(data, { layer: layer.id });
      else if (dragged !== layer.id) void edit({ op: "layerGroup", layer: dragged, group: layer.group ?? null, to: at });
    },
  });
  const onContextMenu = (event: MouseEvent) => {
    event.preventDefault();
    if (!useVj.getState().picked.includes(layer.id)) selectLayer(layer.id);
    const items = [...menu, "-" as const, { heading: t("vj.group.menu") }, ...groupItems(t, grid, layer)];
    openMenu({ x: event.clientX, y: event.clientY, items });
  };
  const select = (event: MouseEvent) => selectLayer(layer.id, event.ctrlKey || event.metaKey);
  if (layer.collapsed) {
    // Folded as in Resolume: one thin row with the name, the clip on air, bypass, solo and the side.
    return (
      <div className={`${className} folded`} onClick={select} onContextMenu={onContextMenu} ref={dropping}>
        <button type="button" className="vj-eject" {...mappable({ kind: "layerStop", layer: layer.id })} title={t("vj.layer.eject")} aria-label={t("vj.layer.eject")}
          onClick={(event) => { event.stopPropagation(); void stopLayer(layer.id); }}>
          <CloseIcon />
        </button>
        {bs}
        {nameRow}
      </div>
    );
  }
  return (
    <div
      className={className}
      onClick={select}
      onContextMenu={onContextMenu}
      ref={dropping}
    >
      <button type="button" className="vj-eject" {...mappable({ kind: "layerStop", layer: layer.id })} title={t("vj.layer.eject")} aria-label={t("vj.layer.eject")}
        onClick={(event) => { event.stopPropagation(); void stopLayer(layer.id); }}>
        <CloseIcon />
      </button>
      <div className="vj-bsb">
        {bs}
        <select className="vj-select" value={layer.blend ?? "alpha"} title={t("vj.layer.blend")} aria-label={t("vj.layer.blend")}
          onChange={(event) => void edit({ op: "layerBlend", layer: layer.id, blend: event.target.value as VjBlend })}>
          <BlendOptions />
        </select>
      </div>
      <div className="vj-faders">
        <Fader letter="M" map={{ kind: "layerMaster", layer: layer.id }} label={t("vj.fader.master")} value={layer.master ?? 1} {...level("layerMaster")}
          modulated={layer.modulation?.master ? { layer: layer.id, field: "master" } : undefined} />
        <Fader letter="A" map={{ kind: "layerVolume", layer: layer.id }} label={t("vj.fader.audio")} value={layer.volume ?? 1} {...level("layerVolume")}
          modulated={layer.modulation?.volume ? { layer: layer.id, field: "volume" } : undefined} />
        <Fader letter="V" map={{ kind: "layerOpacity", layer: layer.id }} label={t("vj.fader.video")} value={layer.opacity ?? 1} {...level("layerOpacity")}
          modulated={layer.modulation?.opacity ? { layer: layer.id, field: "opacity" } : undefined} />
      </div>
      <Thumb className={`vj-now${clip ? "" : " empty"}`} strip={stripOf(thumbs, clip)} generator={clip?.generator}>
        {clip && <PlayTime layer={layer.id} countdown="header" />}
        {transition != null && <span className="vj-tbar"><i style={{ width: `${transition * 100}%` }} /></span>}
      </Thumb>
      <div className="vj-tcol">
        <Fader letter="T" map={{ kind: "layerTransition", layer: layer.id }} label={t("vj.fader.transition")} value={(layer.transition ?? 0) / MAX_TRANSITION}
          progress={transition} {...level("layerTransition", MAX_TRANSITION)} />
        <select className="vj-select short" value={layer.transitionEffect ?? "alpha"} title={t("vj.layer.transitionEffect")}
          aria-label={t("vj.layer.transitionEffect")}
          onChange={(event) => void edit({ op: "layerTransitionEffect", layer: layer.id, effect: event.target.value as VjBlend })}>
          <BlendOptions />
        </select>
      </div>
      {nameRow}
    </div>
  );
}

function Cell({ layer, column, at }: { layer: VjLayer; column: VjColumn; at: number }) {
  const t = useT();
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const thumbs = useDoc((doc) => doc.thumbs);
  const [over, setOver] = useState(false);
  const clip = clipAt(layer, column, at);
  // Only the cell whose clip plays, or is selected, draws again when that changes.
  const selected = useVj((vj) => clip !== undefined && vj.clip === clip.id);
  const live = useVj((vj) => clip !== undefined && vj.play.playing.some((p) => p.layer === layer.id && p.clip === clip.id));
  const state = useVideoState(clip?.asset);

  const dropping = useDropTarget({
    accepts: (data) => DRAG in data || (clip !== undefined && carriesEffect(data, { clip: clip.id })),
    over: setOver,
    drop: (data) => {
      // An effect lands on the clip of the cell, and on nothing in an empty one.
      if (clip && dropEffect(data, { clip: clip.id })) return;
      if (!(DRAG in data)) return;
      const dragged = JSON.parse(data[DRAG]) as Dragged;
      if ("clip" in dragged) {
        if (dragged.clip !== clip?.id) void edit({ op: "moveClip", clip: dragged.clip, layer: layer.id, column: column.id });
      } else if ("generator" in dragged) {
        void edit({ op: "placeGenerator", generator: { kind: dragged.generator }, layer: layer.id, column: column.id });
      } else void edit({ op: "placeVideo", asset: dragged.asset, layer: layer.id, column: column.id });
    },
  });
  // A file from the file explorer is over this cell.
  const filed = useFileDrop((drop) => drop.cell?.layer === layer.id && drop.cell.column === column.id);
  const lit = over || filed;

  if (!clip) {
    // As on a media server, an empty cell stops its layer, over the layer's transition.
    return (
      <div className={`vj-slot${lit ? " over" : ""}`} title={t("vj.cell.empty")} data-drop-layer={layer.id} data-drop-column={column.id}
        onPointerDown={(event) => { if (event.button === 0) void stopLayer(layer.id); }} ref={dropping} />
    );
  }
  const name = clipName(t, clip, videos);
  const piano = clip.trigger === "piano";
  const motion = transportBadge(clip);
  return (
    <div
      className={`vj-clip${live ? " on-air" : ""}${selected ? " sel" : ""}${lit ? " over" : ""}${state === "offline" ? " offline" : ""}`}
      {...mappable({ kind: "clipLaunch", clip: clip.id })}
      data-drop-layer={layer.id} data-drop-column={column.id}
      ref={dropping}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        startDrag(event, { [DRAG]: JSON.stringify({ clip: clip.id } satisfies Dragged) }, name);
        // An offline clip has nothing to play: its click opens the media page on it (a drag swallows the click).
        if (state !== "offline") void launch(clip.id, layer.id);
      }}
      onClick={() => { if (state === "offline" && clip.asset) focusMedia(clip.asset); }}
      onPointerUp={() => { if (piano) void release(clip.id); }}
      onPointerLeave={(event) => { if (piano && event.buttons === 1) void release(clip.id); }}
      onContextMenu={(event) => {
        event.preventDefault();
        selectClip(clip.id, layer.id);
        openMenu({ x: event.clientX, y: event.clientY, items: [
          { label: t("vj.clip.remove"), run: () => void edit({ op: "removeClip", clip: clip.id }) },
        ] });
      }}
    >
      <Thumb strip={stripOf(thumbs, clip)} generator={clip.generator} />
      <span className="vj-shade" />
      <EffectBadge count={clip.effects?.length ?? 0} />
      {live && <PlayTime layer={layer.id} clip={clip.id} countdown="cell" />}
      {state && <VideoState state={state} asset={clip.asset} />}
      {clip.asset && <MediaProgressBar asset={clip.asset} />}
      {/* A folded row has no room to click the cell: its square launches the clip, as the cell does (the pointer reaches the cell). */}
      {layer.collapsed && (
        <button type="button" className={`vj-go${live ? " on" : ""}`} aria-pressed={live}
          title={t("vj.clip.launchNamed", { clip: name })} aria-label={t("vj.clip.launchNamed", { clip: name })}
          onKeyDown={(event) => { if ((event.key === "Enter" || event.key === " ") && !event.repeat) { event.preventDefault(); void launch(clip.id, layer.id); } }}
          onKeyUp={(event) => { if (piano && (event.key === "Enter" || event.key === " ")) void release(clip.id); }} />
      )}
      {/* The name strip selects the clip without launching it. */}
      <div className="vj-pick" aria-label={t("vj.clip.select")}
        onPointerDown={(event) => { event.stopPropagation(); if (event.button === 0) selectClip(clip.id, layer.id); }}
        onPointerUp={(event) => event.stopPropagation()}>
        <Marquee className="vj-cn">{name}</Marquee>
        <span className="vj-cm">{t(`vj.trigger.short.${clip.trigger ?? "normal"}`)}{clip.playback && clip.playback !== "loop" ? ` · ${t(`vj.playback.short.${clip.playback}`)}` : ""}{motion ? ` · ${motion}` : ""}</span>
      </div>
    </div>
  );
}

/** What shows on a cell of a clip that does not play forwards at its own speed: ◀ or ❚❚, then × the speed or the beats of a pass. */
function transportBadge(clip: VjClip): string {
  const parts: string[] = [];
  if (clip.direction === "reverse") parts.push("◀");
  if (clip.direction === "pause") parts.push("❚❚");
  if (clip.tempo?.kind === "beats") parts.push(`${clip.tempo.beats}♩`);
  else if (clip.speed !== undefined && clip.speed !== 1) parts.push(`×${Math.round(clip.speed * 100) / 100}`);
  return parts.join(" ");
}

function ColumnHead({ column, at, live }: { column: VjColumn; at: number; live: boolean }) {
  const t = useT();
  const [renaming, setRenaming] = useState(false);
  return (
    <button
      type="button"
      className={`vj-col${live ? " on-air" : ""}`}
      {...mappable({ kind: "columnLaunch", column: column.id })}
      title={t("vj.column.launch")}
      onClick={() => void launchColumn(column.id)}
      onDoubleClick={() => setRenaming(true)}
      onContextMenu={(event) => {
        event.preventDefault();
        openMenu({ x: event.clientX, y: event.clientY, items: [
          { label: t("vj.column.rename"), run: () => setRenaming(true) },
          { label: t("vj.column.remove"), run: () => void edit({ op: "removeColumn", column: column.id }) },
        ] });
      }}
    >
      <span className="vj-num">{at + 1}</span>
      {renaming ? (
        <InlineName value={column.name ?? ""} label={t("vj.column.rename")} onDone={() => setRenaming(false)}
          onCommit={(text) => void edit({ op: "renameColumn", column: column.id, name: text })} />
      ) : (
        <Marquee>{column.name ?? ""}</Marquee>
      )}
    </button>
  );
}

function DeckTab({ deck, at, shown, only }: { deck: VjDeck; at: number; shown: boolean; only: boolean }) {
  const t = useT();
  const [renaming, setRenaming] = useState(false);
  const name = deck.name ?? t("vj.deck.default", { n: at + 1 });
  return (
    <button
      type="button"
      role="tab"
      aria-selected={shown}
      className={`vj-deck${shown ? " on" : ""}`}
      {...mappable({ kind: "deckSelect", deck: deck.id })}
      onClick={() => { if (!shown) void edit({ op: "selectDeck", deck: deck.id }); }}
      onDoubleClick={() => setRenaming(true)}
      onContextMenu={(event) => {
        event.preventDefault();
        openMenu({ x: event.clientX, y: event.clientY, items: [
          { label: t("vj.deck.rename"), run: () => setRenaming(true) },
          { label: t("vj.deck.remove"), disabled: only, run: () => void edit({ op: "removeDeck", deck: deck.id }) },
        ] });
      }}
    >
      {renaming ? (
        <InlineName value={deck.name ?? ""} label={t("vj.deck.rename")} onDone={() => setRenaming(false)}
          onCommit={(text) => void edit({ op: "renameDeck", deck: deck.id, name: text })} />
      ) : <Marquee className="vj-dname">{name}</Marquee>}
      <span className="vj-count">{deck.columns.length}</span>
    </button>
  );
}

function Crossfader() {
  const position = useVj((vj) => vj.play.crossfader);
  const [held, setHeld] = useState<number | null>(null);
  const shown = held ?? position;
  const at = (event: PointerEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return Math.min(1, Math.max(0, (event.clientX - box.left) / box.width));
  };
  const move = (v: number) => {
    setHeld(v);
    void moveCrossfader(v);
  };
  return (
    <div
      className="vj-xt"
      {...mappable({ kind: "crossfader" })}
      role="slider"
      tabIndex={0}
      aria-label="Crossfader"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(shown * 100)}
      onPointerDown={(event) => { event.currentTarget.setPointerCapture(event.pointerId); move(at(event)); }}
      onPointerMove={(event) => { if (held !== null) move(at(event)); }}
      onPointerUp={() => setHeld(null)}
      onPointerCancel={() => setHeld(null)}
      onDoubleClick={() => void moveCrossfader(0.5)}
    >
      <span className="vj-tick" style={{ left: 0 }} />
      <span className="vj-tick mid" />
      <span className="vj-tick" style={{ left: "100%" }} />
      <b style={{ left: `${shown * 100}%` }} />
    </div>
  );
}

/** The header row of a group: its switches, blend and faders, and a trigger per column that launches it on the group's layers only. */
function GroupRow({ grid, group, deck }: { grid: VjGrid; group: VjGroup; deck: VjDeck }) {
  const t = useT();
  const [renaming, setRenaming] = useState(false);
  const [over, setOver] = useState(false);
  const selected = useVj((vj) => vj.tab === "group" && vj.group === group.id);
  const name = groupName(t, grid, group);
  const count = grid.layers.filter((layer) => layer.group === group.id).length;
  const at = (grid.groups ?? []).indexOf(group);
  const level = (op: "groupMaster" | "groupOpacity") => ({
    onMove: (value: number) => preview({ op, group: group.id, value }),
    onSet: (value: number) => void edit({ op, group: group.id, value }),
  });
  const fold = () => void edit({ op: "groupCollapse", group: group.id, on: !group.collapsed });
  const ungroup = () => void edit({ op: "ungroup", group: group.id });
  const foldKey = group.collapsed ? "vj.group.expand" : "vj.group.collapse";
  const menu: MenuItem[] = [
    { label: t("vj.group.rename"), run: () => setRenaming(true) },
    { label: t(foldKey), run: fold },
    "-",
    { label: t("vj.group.ungroup"), run: ungroup },
  ];
  const dropping = useDropTarget({
    accepts: (data) => DRAG_LAYER in data || carriesEffect(data, { group: group.id }),
    over: setOver,
    drop: (data) => {
      const dragged = data[DRAG_LAYER];
      if (!dragged) dropEffect(data, { group: group.id });
      else void edit({ op: "layerGroup", layer: dragged, group: group.id, to: null });
    },
  });
  const bs = (
    <div className="vj-bs">
      <button type="button" className={group.bypass ? "on bypass" : ""} {...mappable({ kind: "groupBypass", group: group.id })} aria-pressed={!!group.bypass} title={t("vj.group.bypass")}
        onClick={(event) => { event.stopPropagation(); void edit({ op: "groupBypass", group: group.id, on: !group.bypass }); }}>B</button>
      <button type="button" className={group.solo ? "on solo" : ""} {...mappable({ kind: "groupSolo", group: group.id })} aria-pressed={!!group.solo} title={t("vj.group.solo")}
        onClick={(event) => { event.stopPropagation(); void edit({ op: "groupSolo", group: group.id, on: !group.solo }); }}>S</button>
    </div>
  );
  const nameRow = (
    <div className="vj-lname">
      <button type="button" className="vj-fold" aria-expanded={!group.collapsed} title={t(foldKey)} aria-label={t(foldKey)}
        onClick={(event) => { event.stopPropagation(); fold(); }}>
        {group.collapsed ? <ChevronRightIcon /> : <ChevronDownIcon />}
      </button>
      <span className="vj-num">G{at + 1}</span>
      {renaming ? (
        <InlineName value={group.name ?? ""} label={t("vj.group.rename")} onDone={() => setRenaming(false)}
          onCommit={(text) => void edit({ op: "renameGroup", group: group.id, name: text })} />
      ) : (
        <Marquee className="vj-nm" title={name} onDoubleClick={() => setRenaming(true)}>{name}</Marquee>
      )}
      <EffectBadge count={group.effects?.length ?? 0} />
      <span className="vj-clipname idle">{t(count === 1 ? "vj.group.layer" : "vj.group.layers", { n: count })}</span>
    </div>
  );
  const className = `vj-lh group${group.collapsed ? " folded" : ""}${over ? " over" : ""}${selected ? " selected" : ""}`;
  const onContextMenu = (event: MouseEvent) => {
    event.preventDefault();
    openMenu({ x: event.clientX, y: event.clientY, items: menu });
  };
  return (
    <div className={`vj-row group${group.collapsed ? " folded" : ""}`}>
      {group.collapsed ? (
        <div className={className} onClick={() => selectGroup(group.id)} onContextMenu={onContextMenu} ref={dropping}>
          {bs}
          {nameRow}
        </div>
      ) : (
        <div className={className} onClick={() => selectGroup(group.id)} onContextMenu={onContextMenu} ref={dropping}>
          <button type="button" className="vj-eject" title={t("vj.group.ungroup")} aria-label={t("vj.group.ungroup")}
            onClick={(event) => { event.stopPropagation(); ungroup(); }}>
            <CloseIcon />
          </button>
          <div className="vj-bsb">
            {bs}
            <select className="vj-select" value={group.blend ?? "alpha"} title={t("vj.group.blend")} aria-label={t("vj.group.blend")}
              onClick={(event) => event.stopPropagation()}
              onChange={(event) => void edit({ op: "groupBlend", group: group.id, blend: event.target.value as VjBlend })}>
              <BlendOptions />
            </select>
          </div>
          <div className="vj-faders two">
            <Fader letter="M" map={{ kind: "groupMaster", group: group.id }} label={t("vj.fader.master")} value={group.master ?? 1} {...level("groupMaster")} />
            <Fader letter="V" map={{ kind: "groupOpacity", group: group.id }} label={t("vj.fader.video")} value={group.opacity ?? 1} {...level("groupOpacity")} />
          </div>
          {nameRow}
        </div>
      )}
      {deck.columns.map((column, place) => (
        <button key={column.id} type="button" className="vj-gcell" title={t("vj.group.launch", { n: place + 1, name })} aria-label={t("vj.group.launch", { n: place + 1, name })}
          onPointerDown={(event) => { if (event.button === 0) void launchGroupColumn(group.id, column.id); }}>
          <PlayIcon />
        </button>
      ))}
    </div>
  );
}

function DeckPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const deck = grid.decks.find((d) => d.id === grid.deck) ?? grid.decks[0];
  const layers = [...grid.layers].reverse();
  const lit = useVj((vj) => vj.play.column);
  const liveColumn = deck.columns.findIndex((column) => column.id === lit);
  const sideNames = (side: VjSide) => grid.layers.filter((l) => l.crossfade === side).map((l) => layerName(t, grid, l)).join(", ");
  const columns = { "--columns": deck.columns.length } as CSSProperties;
  // An effect dropped on the composition bar goes to the composition's stack.
  const composition = useCompositionDrop();
  const mapping = useMap((state) => state.on);
  return (
    <section className="vj-panel vj-deck-panel" aria-label={t("vj.grid")}>
      <header className="vj-ph">
        <h2>{t("vj.grid")}</h2>
        {mapping && <MapBar grid={grid} />}
        <div className="vj-tools">
          <button type="button" className="vj-tb" onClick={() => void edit({ op: "addLayer", global: false })}><PlusIcon />{t("vj.layer.add")}</button>
          <button type="button" className="vj-tb" onClick={() => void edit({ op: "addLayer", global: true })}><GlobeIcon />{t("vj.layer.addGlobal")}</button>
          <button type="button" className="vj-tb" onClick={() => void edit({ op: "addColumn" })}><PlusIcon />{t("vj.column.add")}</button>
        </div>
      </header>
      <div className="vj-pb">
        <div className="vj-grid" style={columns}>
          <div className="vj-row head">
            <div className="vj-corner">{t("vj.summary", { layers: grid.layers.length, columns: deck.columns.length })}</div>
            {deck.columns.map((column, at) => <ColumnHead key={column.id} column={column} at={at} live={at === liveColumn} />)}
          </div>
          {layers.flatMap((layer, i) => {
            const group = grid.groups?.find((g) => g.id === layer.group);
            const rows: ReactNode[] = [];
            // A group's header row comes over its first layer; a folded group shows nothing else.
            if (group && layers[i - 1]?.group !== layer.group) rows.push(<GroupRow key={`group-${group.id}`} grid={grid} group={group} deck={deck} />);
            if (group?.collapsed) return rows;
            rows.push(
              <div key={layer.id} className={`vj-row${layer.global ? " global" : ""}${layer.collapsed ? " folded" : ""}${group ? " in-group" : ""}${i > 0 && layers[i - 1].global && !layer.global ? " split" : ""}`}>
                <LayerHeader grid={grid} layer={layer} />
                {deck.columns.map((column, at) => (
                  <Cell key={column.id} layer={layer} column={column} at={at} />
                ))}
              </div>,
            );
            return rows;
          })}
        </div>
        <div className="vj-decks">
          <div className="vj-xf" title={t("vj.crossfader.sides", { a: sideNames("a") || "—", b: sideNames("b") || "—" })}>
            <b>A</b>
            <Crossfader />
            <b>B</b>
          </div>
          <div className="vj-deck-tabs" role="tablist" aria-label={t("vj.decks")}>
            <button type="button" className="vj-deck step" {...mappable({ kind: "deckPrev" })} title={t("vj.menu.previousDeck")} aria-label={t("vj.menu.previousDeck")} onClick={() => stepDeck(-1)}>‹</button>
            <button type="button" className="vj-deck step" {...mappable({ kind: "deckNext" })} title={t("vj.menu.nextDeck")} aria-label={t("vj.menu.nextDeck")} onClick={() => stepDeck(1)}>›</button>
            {grid.decks.map((d, at) => <DeckTab key={d.id} deck={d} at={at} shown={d.id === deck.id} only={grid.decks.length === 1} />)}
            <button type="button" className="vj-deck add" title={t("vj.deck.add")} aria-label={t("vj.deck.add")}
              onClick={() => void edit({ op: "addDeck" })}><PlusIcon /></button>
          </div>
        </div>
        <div className={`vj-comp${composition.over ? " over" : ""}`} ref={composition.ref}>
          <div className="vj-cl"><strong>{t("vj.composition")}</strong><AnalysisMeter /><CompositionFx grid={grid} /></div>
          <Tempo />
          <TimecodeClock grid={grid} />
          <button type="button" className="button" {...mappable({ kind: "stopAll" })} onClick={() => void stopLayer(null)}><StopIcon />{t("vj.stopAll")}</button>
        </div>
      </div>
    </section>
  );
}

/** What the screens show, as the backend draws it (see `bindOutput`). */
function OutputPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const composition = useCompositionDrop();
  const live = useVj((vj) => vj.play.playing.some((p) => p.level > 0));
  const disabled = useOutput((output) => output.disabled);
  const testCard = useOutput((output) => output.testCard);
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => bindOutput((picture) => {
    const target = canvas.current;
    const context = target?.getContext("2d");
    if (target && context) {
      if (picture && (target.width !== picture.width || target.height !== picture.height)) {
        target.width = picture.width;
        target.height = picture.height;
      }
      if (picture) context.putImageData(picture, 0, 0);
      else context.clearRect(0, 0, target.width, target.height);
    }
  }), []);
  return (
    <section className={`vj-panel${composition.over ? " over" : ""}`} aria-label={t("vj.output")} ref={composition.ref}>
      <header className="vj-ph">
        <h2>{t("vj.output")}</h2>
        <div className="vj-tools">
          <CompositionFx grid={grid} />
          {disabled && <button type="button" className="vj-pill off" title={t("vj.output.reenable")} onClick={toggleDisable}><i />{t("vj.output.disabled")}</button>}
          {testCard && !disabled && <span className="vj-pill card"><i />{t("vj.output.testCard")}</span>}
          {live && <span className="vj-pill"><i />{t("vj.onAir")}</span>}
        </div>
      </header>
      <div className="vj-pb vj-mon-wrap">
        <div className="vj-mon" data-disabled={disabled}>
          <canvas ref={canvas} className="vj-out" />
          {disabled && <p className="vj-mon-off" role="status">{t("vj.output.disabledNote")}</p>}
        </div>
      </div>
    </section>
  );
}

function PreviewPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const selected = useVj((vj) => vj.clip);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const found = grid.layers.flatMap((layer) => (layer.clips ?? []).map((clip) => ({ layer, clip }))).find((x) => x.clip.id === selected);
  const canvas = useRef<HTMLCanvasElement>(null);
  const id = found?.clip.id;
  useEffect(() => {
    if (!id) return;
    return bindPreview(id, (picture) => {
      const target = canvas.current;
      const context = target?.getContext("2d");
      if (!target || !context) return;
      if (picture && (target.width !== picture.width || target.height !== picture.height)) {
        target.width = picture.width;
        target.height = picture.height;
      }
      if (picture) context.putImageData(picture, 0, 0);
      else context.clearRect(0, 0, target.width, target.height);
    });
  }, [id]);
  return (
    <section className="vj-panel" aria-label={t("vj.preview")}>
      <header className="vj-ph">
        <h2>{t("vj.preview")}</h2>
        <div className="vj-tools">{found && <Marquee className="vj-dim">{clipName(t, found.clip, videos)}</Marquee>}</div>
      </header>
      <div className="vj-pb vj-mon-wrap">
        <div className="vj-mon">
          {found ? <canvas ref={canvas} className="vj-out" /> : <p className="vj-empty">{t("vj.previewEmpty")}</p>}
        </div>
        {found && (
          <div className="vj-acts">
            <button type="button" className="button primary" onClick={() => void launch(found.clip.id, found.layer.id)}>
              <PlayIcon />{t("vj.launchOn", { layer: layerName(t, grid, found.layer) })}
            </button>
          </div>
        )}
      </div>
    </section>
  );
}

/** A name typed in place; one the backend refuses goes back to the name it had. */
function NameField({ value, placeholder, label, onCommit }: { value: string; placeholder: string; label: string; onCommit: (name: string) => Promise<boolean> }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <input className="vj-input" value={text} placeholder={placeholder} aria-label={label}
      onChange={(event) => setText(event.target.value)}
      onBlur={() => {
        if (text !== value) void onCommit(text).then((done) => { if (!done) setText(value); });
      }}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Enter") event.currentTarget.blur();
        if (event.key === "Escape") { setText(value); event.currentTarget.blur(); }
      }} />
  );
}

/** The parameters of a generator clip, each with the backend's default where the clip has none. */
function GeneratorFields({ clip, generator, modulation }: { clip: string; generator: VjGenerator; modulation: VjClip["modulation"] }) {
  const t = useT();
  const move = (changed: Partial<VjGenerator>) =>
    preview({ op: "clipGenerator", clip, generator: { ...generator, ...changed } as VjGenerator });
  const keep = (changed: Partial<VjGenerator>) =>
    void edit({ op: "clipGenerator", clip, generator: { ...generator, ...changed } as VjGenerator });
  const colour = (key: "colour" | "from" | "to", value: string, name: string) => (
    <div className="vj-f"><span>{name}</span>
      <ColourField value={value} label={name} onMove={(c) => move({ [key]: c })} onSet={(c) => keep({ [key]: c })} />
    </div>
  );
  const number = (key: "rate" | "width" | "angle", value: number, props: { label: string; name: string; shown: number; reset: number; unit: string; decimals?: number; hint?: string }) => (
    <>
      <div className="vj-f" title={props.hint}><span>{props.name}</span>
        <div className="vj-scrubs one">
          <GeneratorScrub {...props} clip={clip} modulation={modulation} field={key} value={value} onMove={(v) => move({ [key]: v })} onSet={(v) => keep({ [key]: v })} />
        </div>
      </div>
      <GeneratorModPanel clip={clip} modulation={modulation} field={key} shown={props.shown} unit={props.unit} decimals={props.decimals} />
    </>
  );
  switch (generator.kind) {
    case "solid":
      return colour("colour", generator.colour ?? "#ffffff", t("vj.gen.colour"));
    case "strobe":
      return (
        <>
          {colour("colour", generator.colour ?? "#ffffff", t("vj.gen.colour"))}
          {number("rate", generator.rate ?? 8, { label: "↻", name: t("vj.gen.rate"), shown: 1, reset: 8, unit: "Hz", decimals: 1, hint: t("vj.gen.rateHint") })}
          {number("width", generator.width ?? 0.5, { label: "▮", name: t("vj.gen.width"), shown: 100, reset: 0.5, unit: "%", hint: t("vj.gen.widthHint") })}
        </>
      );
    case "gradient":
      return (
        <>
          {colour("from", generator.from ?? "#000000", t("vj.gen.from"))}
          {colour("to", generator.to ?? "#ffffff", t("vj.gen.to"))}
          {number("angle", generator.angle ?? 0, { label: "∠", name: t("vj.gen.angle"), shown: 1, reset: 0, unit: "°" })}
        </>
      );
    case "noise":
      return (
        <>
          {colour("colour", generator.colour ?? "#ffffff", t("vj.gen.colour"))}
          {number("rate", generator.rate ?? 30, { label: "↻", name: t("vj.gen.rate"), shown: 1, reset: 30, unit: "Hz", decimals: 1, hint: t("vj.gen.noiseRateHint") })}
        </>
      );
    case "text":
      return <TextFields clip={clip} generator={generator} modulation={modulation} />;
    case "bars":
      return null;
  }
}

/** The levels of a layer the inspector sets and can modulate: the same as its master, audio and video faders. */
const LEVELS = [
  { field: "master", op: "layerMaster", label: "vj.fader.master", tag: "M" },
  { field: "volume", op: "layerVolume", label: "vj.fader.audio", tag: "A" },
  { field: "opacity", op: "layerOpacity", label: "vj.fader.video", tag: "V" },
] as const;

function Inspector({ grid }: { grid: VjGrid }) {
  const t = useT();
  // One value each, so the play state polled twenty times a second does not draw the inspector again.
  const clipId = useVj((vj) => vj.clip);
  const layerId = useVj((vj) => vj.layer);
  const groupId = useVj((vj) => vj.group);
  const tab = useVj((vj) => vj.tab);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const probes = useDoc((doc) => doc.video);
  const thumbs = useDoc((doc) => doc.thumbs);
  const found = grid.layers.flatMap((layer) => (layer.clips ?? []).map((clip) => ({ layer, clip }))).find((x) => x.clip.id === clipId);
  const layer = grid.layers.find((l) => l.id === layerId) ?? found?.layer;
  const group = grid.groups?.find((g) => g.id === groupId);
  const modCatalog = useEffectCatalog((state) => state.catalog?.modulation);
  // The modulated values are asked for only while the inspector or a layer's faders show something modulated.
  const modulated =
    inspectorModulated(layer, found?.clip, grid.compositionEffects) || grid.layers.some((l) => inspectorModulated(l, undefined)) || (grid.dashboard ?? []).some((knob) => knob.modulation);
  useEffect(() => (modulated ? bindModulation() : undefined), [modulated]);
  const tabs = (
    <nav className="vj-tabs" role="tablist">
      {(["clip", "layer", "group", "composition", "mappings"] as const).map((key) => (
        <button key={key} type="button" role="tab" aria-selected={tab === key} className={tab === key ? "on" : ""} onClick={() => showTab(key)}>
          {t(`vj.inspector.${key}`)}
        </button>
      ))}
    </nav>
  );
  let body: ReactNode = <p className="vj-empty">{t(tab === "clip" ? "vj.inspector.noClip" : tab === "group" ? "vj.inspector.noGroup" : "vj.inspector.noLayer")}</p>;
  if (tab === "clip" && found) {
    const { clip } = found;
    const { generator } = clip;
    const video = videos.find((v) => v.id === clip.asset);
    const probe = clip.asset ? probes[clip.asset] : undefined;
    const deck = grid.decks.find((d) => d.id === grid.deck);
    const scale = clip.scale ?? 1;
    const [x, y] = clip.position ?? [0, 0];
    const columnAt = found.layer.global ? clip.slot ?? 0 : deck?.columns.findIndex((c) => c.id === clip.column) ?? -1;
    // A transform number that can be modulated, with its "~" button; the row of `scale` is drawn here, the two of
    // `position` by the caller. The sub-rows go under them.
    const modScrub = (field: string, name: string, tag: string, value: number, reset: number, onMove: (v: number) => void, onSet: (v: number) => void, bare = false) => {
      const range = fieldRange(modCatalog, "clip", field);
      const scrub = { label: tag, name, value, shown: 100, reset, onMove, onSet };
      const control = range
        ? <ModScrub key={field} {...scrub} target={{ clip: clip.id, field }} modulation={clip.modulation?.[field]} range={range} />
        : <Scrub key={field} {...scrub} />;
      return bare ? control : <div className="vj-f"><span>{name}</span><div className="vj-scrubs one">{control}</div></div>;
    };
    const modPanel = (field: string) => {
      const range = fieldRange(modCatalog, "clip", field);
      return range && <ModPanel key={field} target={{ clip: clip.id, field }} modulation={clip.modulation?.[field]} range={range} />;
    };
    body = (
      <div className="vj-insp">
        <Transport clip={clip} layer={found.layer.id} strip={stripOf(thumbs, clip)} still={probe?.still} />
        <Thumb className="vj-ithumb" strip={stripOf(thumbs, clip)} generator={generator} />
        <div className="vj-fields">
          <label className="vj-f"><span>{t("vj.clip.name")}</span>
            <NameField value={clip.name ?? ""} placeholder={generator ? t(`vj.generator.${generator.kind}`) : video?.name ?? ""} label={t("vj.clip.name")}
              onCommit={(name) => edit({ op: "renameClip", clip: clip.id, name })} />
          </label>
          <div className="vj-f"><span>{t("vj.clip.trigger")}</span>
            <Segments options={TRIGGERS} value={clip.trigger ?? "normal"} label={(o) => t(`vj.trigger.${o}`)}
              onPick={(trigger) => void edit({ op: "clipTrigger", clip: clip.id, trigger })} />
          </div>
          {generator && <GeneratorFields clip={clip.id} generator={generator} modulation={clip.modulation} />}
          <div className="vj-f"><span>{t("vj.clip.playback")}</span>
            <Segments options={PLAYBACKS} value={clip.playback ?? "loop"} label={(o) => t(`vj.playback.${o}`)}
              onPick={(playback) => void edit({ op: "clipPlayback", clip: clip.id, playback })} />
          </div>
          {!generator && (
            <div className="vj-f"><span>{t("vj.clip.fit")}</span>
              <Segments options={FITS} value={clip.fit ?? "fit"} label={(o) => t(`vj.fit.${o}`)}
                onPick={(fit) => void edit({ op: "clipFit", clip: clip.id, fit })} />
            </div>
          )}
          {modScrub("scale", t("vj.clip.scale"), "↔", scale, 1, (v) => preview({ op: "clipScale", clip: clip.id, value: v }),
            (v) => void edit({ op: "clipScale", clip: clip.id, value: v }))}
          {modPanel("scale")}
          <div className="vj-f"><span>{t("vj.clip.position")}</span>
            <div className="vj-scrubs">
              {modScrub("positionX", t("vj.clip.x"), "X", x, 0, (v) => preview({ op: "clipPosition", clip: clip.id, x: v, y }),
                (v) => void edit({ op: "clipPosition", clip: clip.id, x: v, y }), true)}
              {modScrub("positionY", t("vj.clip.y"), "Y", y, 0, (v) => preview({ op: "clipPosition", clip: clip.id, x, y: v }),
                (v) => void edit({ op: "clipPosition", clip: clip.id, x, y: v }), true)}
            </div>
          </div>
          {(["positionX", "positionY"] as const).map((field) => modPanel(field))}
        </div>
        <EffectStack target={{ clip: clip.id }} effects={clip.effects ?? NO_EFFECTS} />
        <dl className="vj-meta">
          {generator ? (
            <div><dt>{t("vj.gen.kind")}</dt><dd>{t(`vj.generator.${generator.kind}`)}</dd></div>
          ) : (
            <>
              <div><dt>{t("vj.clip.file")}</dt><dd><Marquee>{video?.name ?? ""}</Marquee></dd></div>
              {video && <MediaFile id={video.id} />}
              <div><dt>{t("vj.clip.length")}</dt><dd className="vj-mono">{!probe ? "—" : probe.still ? t("vj.media.still") : clock(probe.frames * probe.rateDen / probe.rateNum)}</dd></div>
              <div><dt>{t("vj.clip.video")}</dt><dd className="vj-mono">{!probe ? "—" : probe.still ? `${probe.codec} · ${probe.width}×${probe.height}` : `${probe.codec} · ${probe.width}×${probe.height} · ${Math.round((probe.rateNum / probe.rateDen) * 100) / 100}`}</dd></div>
            </>
          )}
          <div><dt>{t("vj.clip.cell")}</dt><dd><Marquee>{layerName(t, grid, found.layer)} · {columnAt + 1}</Marquee></dd></div>
        </dl>
      </div>
    );
  } else if (tab === "layer" && layer) {
    body = (
      <div className="vj-insp single">
        <div className="vj-fields">
          <label className="vj-f"><span>{t("vj.layer.name")}</span>
            <NameField value={layer.name ?? ""} placeholder={t("vj.layer.default", { n: grid.layers.indexOf(layer) + 1 })} label={t("vj.layer.name")}
              onCommit={(name) => edit({ op: "renameLayer", layer: layer.id, name })} />
          </label>
          <label className="vj-f"><span>{t("vj.layer.blend")}</span>
            <select className="vj-select wide" value={layer.blend ?? "alpha"}
              onChange={(event) => void edit({ op: "layerBlend", layer: layer.id, blend: event.target.value as VjBlend })}>
              <BlendOptions />
            </select>
          </label>
          <label className="vj-f"><span>{t("vj.layer.transitionEffect")}</span>
            <select className="vj-select wide" value={layer.transitionEffect ?? "alpha"}
              onChange={(event) => void edit({ op: "layerTransitionEffect", layer: layer.id, effect: event.target.value as VjBlend })}>
              <BlendOptions />
            </select>
          </label>
          <div className="vj-f" {...mappable({ kind: "layerAutopilot", layer: layer.id })}><span>{t("vj.layer.autopilot")}</span>
            <Segments options={AUTOPILOTS} value={layer.autopilot ?? "off"} label={(o) => t(`vj.autopilot.${o}`)}
              onPick={(mode) => void edit({ op: "layerAutopilot", layer: layer.id, mode })} />
          </div>
          <div className="vj-f"><span>{t("vj.layer.transition")}</span>
            <span className="vj-mono">{t("vj.seconds", { n: (layer.transition ?? 0).toFixed(1) })}</span>
          </div>
          <div className="vj-f"><span>{t("vj.layer.kind")}</span>
            <span>{t(layer.global ? "vj.layer.kindGlobal" : "vj.layer.kindDeck")}</span>
          </div>
          {LEVELS.map(({ field, op, label, tag }) => {
            const range = fieldRange(modCatalog, "layer", field);
            const value = layer[field] ?? 1;
            const scrub = {
              label: tag, name: t(label), value, shown: 100, reset: 1,
              onMove: (v: number) => preview({ op, layer: layer.id, value: v }),
              onSet: (v: number) => void edit({ op, layer: layer.id, value: v }),
            };
            return (
              <Fragment key={field}>
                <div className="vj-f"><span>{t(label)}</span>
                  <div className="vj-scrubs one">
                    {range
                      ? <ModScrub {...scrub} target={{ layer: layer.id, field }} modulation={layer.modulation?.[field]} range={range} />
                      : <Scrub {...scrub} />}
                  </div>
                </div>
                {range && <ModPanel target={{ layer: layer.id, field }} modulation={layer.modulation?.[field]} range={range} />}
              </Fragment>
            );
          })}
        </div>
        <EffectStack target={{ layer: layer.id }} effects={layer.effects ?? NO_EFFECTS} />
      </div>
    );
  } else if (tab === "group" && group) {
    body = (
      <div className="vj-insp single">
        <div className="vj-fields">
          <label className="vj-f"><span>{t("vj.group.name")}</span>
            <NameField value={group.name ?? ""} placeholder={t("vj.group.default", { n: (grid.groups ?? []).indexOf(group) + 1 })} label={t("vj.group.name")}
              onCommit={(name) => edit({ op: "renameGroup", group: group.id, name })} />
          </label>
          <label className="vj-f"><span>{t("vj.layer.blend")}</span>
            <select className="vj-select wide" value={group.blend ?? "alpha"}
              onChange={(event) => void edit({ op: "groupBlend", group: group.id, blend: event.target.value as VjBlend })}>
              <BlendOptions />
            </select>
          </label>
          {([["master", "groupMaster", "vj.fader.master", "M"], ["opacity", "groupOpacity", "vj.fader.video", "V"]] as const).map(([field, op, label, tag]) => (
            <div key={field} className="vj-f"><span>{t(label)}</span>
              <div className="vj-scrubs one">
                <Scrub label={tag} name={t(label)} value={group[field] ?? 1} shown={100} reset={1}
                  onMove={(v: number) => preview({ op, group: group.id, value: v })}
                  onSet={(v: number) => void edit({ op, group: group.id, value: v })} />
              </div>
            </div>
          ))}
        </div>
        <p className="vj-dim vj-fx-empty">{t("vj.group.note")}</p>
        <EffectStack target={{ group: group.id }} effects={group.effects ?? NO_EFFECTS} />
      </div>
    );
  } else if (tab === "composition") {
    body = (
      <div className="vj-insp single">
        <p className="vj-dim vj-fx-empty">{t("vj.composition.note")}</p>
        <EffectStack target="composition" effects={grid.compositionEffects ?? NO_EFFECTS} />
        <Dashboard grid={grid} />
      </div>
    );
  } else if (tab === "mappings") {
    body = <MappingsPanel grid={grid} />;
  }
  return (
    <section className="vj-panel vj-insp-panel">
      <header className="vj-ph">{tabs}</header>
      <div className="vj-pb">{body}</div>
    </section>
  );
}

function MediaState({ asset }: { asset: string }) {
  const state = useVideoState(asset);
  return state && <VideoState state={state} asset={asset} />;
}

function MediaPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const probes = useDoc((doc) => doc.video);
  const thumbs = useDoc((doc) => doc.thumbs);
  const layerId = useVj((vj) => vj.layer);
  // The clips playing as one string, so the panel draws again only when they change, not on every poll.
  const playingClips = useVj((vj) => vj.play.playing.map((p) => p.clip).join(" "));
  const target = grid.layers.find((l) => l.id === layerId) ?? grid.layers.find((l) => !l.global) ?? grid.layers[0];
  const onAir = new Set(grid.layers.flatMap((l) =>
    (l.clips ?? []).filter((c) => playingClips.split(" ").includes(c.id)).map((c) => c.asset)));
  const importing = useVj((vj) => vj.importing);
  const kind = useVj((vj) => vj.media);
  const browser = useVj((vj) => vj.browser);
  const selectedClip = useVj((vj) => vj.clip);
  const tab = useVj((vj) => vj.tab);
  // What a double click on an effect adds it to: the composition while its tab is shown, else the clip selected, or else the layer.
  const effectTarget: EffectTarget = tab === "composition"
    ? "composition"
    : grid.layers.some((l) => l.clips?.some((c) => c.id === selectedClip))
      ? { clip: selectedClip as string }
      : { layer: target.id };
  // A video still being read is listed under "All" only, until the backend says what it is.
  const shown = kind === "all" ? videos : videos.filter((video) => {
    const still = probes[video.id]?.still;
    return still !== undefined && still === (kind === "image");
  });
  const importFiles = async () => {
    try {
      await importVideos(await pickMediaPaths(), target.id, null);
    } catch (err) {
      fail(err);
    }
  };
  return (
    <section className="vj-panel vj-media" aria-label={t("vj.media")}>
      <header className="vj-ph">
        <nav className="vj-tabs" role="tablist">
          {(["media", "generators", "effects"] as const).map((key) => (
            <button key={key} type="button" role="tab" aria-selected={browser === key} className={browser === key ? "on" : ""}
              onClick={() => showBrowser(key)}>{t(key === "media" ? "vj.media" : key === "generators" ? "vj.browser.generators" : "vj.effects")}</button>
          ))}
        </nav>
        {browser === "media" && (
          <div className="vj-tools">
            <Segments options={MEDIA_KINDS} value={kind} label={(o) => t(`vj.media.kind.${o}`)} onPick={showMedia} />
            <button type="button" className="vj-tb" title={t("vj.media.importNote", { layer: layerName(t, grid, target) })}
              onClick={() => void importFiles()}><ImportIcon />{t("vj.media.import")}</button>
          </div>
        )}
        {browser === "generators" && <div className="vj-tools"><span className="vj-dim">{t("vj.generators.note")}</span></div>}
        {browser === "effects" && <div className="vj-tools"><span className="vj-dim">{t("vj.effects.note")}</span></div>}
      </header>
      <div className="vj-pb">
        {browser === "effects" ? (
          <EffectBrowser target={effectTarget} />
        ) : browser === "generators" ? (
          <div className="vj-mg">
            {GENERATORS.map((kind) => (
              <figure key={kind} className="vj-m" title={t("vj.generators.note")}
                onPointerDown={(event) => startDrag(event, { [DRAG]: JSON.stringify({ generator: kind } satisfies Dragged) }, t(`vj.generator.${kind}`))}>
                <Thumb strip={undefined} generator={{ kind }} />
                <figcaption><Marquee>{t(`vj.generator.${kind}`)}</Marquee></figcaption>
              </figure>
            ))}
          </div>
        ) : videos.length === 0 && importing.length === 0 ? (
          <p className="vj-empty">{t("vj.media.empty")}</p>
        ) : (
          <div className="vj-mg">
            {shown.length === 0 && importing.length === 0 && <p className="vj-empty">{t("vj.media.none")}</p>}
            {shown.map((video) => {
              const probe = probes[video.id];
              return (
                <figure key={video.id} className={`vj-m${onAir.has(video.id) ? " using" : ""}`}
                  onPointerDown={(event) => startDrag(event, { [DRAG]: JSON.stringify({ asset: video.id } satisfies Dragged) }, video.name)}>
                  <Thumb strip={thumbs[video.id]}>
                    {probe && <span className="vj-d">{probe.still ? t("vj.media.still") : clock(probe.frames * probe.rateDen / probe.rateNum)}</span>}
                    <MediaState asset={video.id} />
                  </Thumb>
                  <figcaption><Marquee>{video.name}</Marquee><span className="vj-c">{probe?.codec ?? ""}</span></figcaption>
                </figure>
              );
            })}
            {importing.map((item) => (
              <figure key={item.key} className="vj-m busy">
                <Thumb strip={undefined}><VideoState state="importing" /></Thumb>
                <figcaption><Marquee>{item.name}</Marquee></figcaption>
              </figure>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

/** The screen of a VJ project: the grid of clips, the output and preview, the inspector and the media. */
export function VjScreen() {
  const grid = useDoc((doc) => doc.project.vj);
  useEffect(bindPlayState, []);
  useEffect(bindVjControl, []);
  useEffect(bindOutputState, []);
  useEffect(() => { void loadEffectCatalog(); void loadPresets(); }, []);
  if (!grid) return null;
  return (
    <div className="vj-app">
      <DeckPanel grid={grid} />
      <div className="vj-right">
        <OutputPanel grid={grid} />
        <PreviewPanel grid={grid} />
      </div>
      <Inspector grid={grid} />
      <MediaPanel grid={grid} />
      <AdvancedOutput />
      <MapLayer grid={grid} />
      <MapImportDialog />
      <NameDialog />
      <LibraryDialog />
    </div>
  );
}

/** The VJ screen's tips, shown at the right of the status bar. */
export function VjTips() {
  const t = useT();
  const global = useDoc((doc) => doc.project.vj?.layers.some((layer) => layer.global) ?? false);
  return (
    <span className="vj-tips">
      {global && <span className="vj-hint"><GlobeIcon />{t("vj.globalHint")}</span>}
      <span>{t("vj.outputNote")}</span>
    </span>
  );
}
