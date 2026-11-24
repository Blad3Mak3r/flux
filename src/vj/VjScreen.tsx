import { useEffect, useRef, useState } from "react";
import type { CSSProperties, DragEvent, KeyboardEvent, PointerEvent, ReactNode } from "react";
import { InlineName } from "../components/InlineName";
import { Marquee } from "../components/Marquee";
import { CloseIcon, GlobeIcon, ImportIcon, PlayIcon, PlusIcon, StopIcon } from "../components/icons";
import { useT } from "../i18n";
import type { Translate } from "../i18n";
import { errorMessage, pickMediaPaths } from "../project/api";
import { useVideoErrors } from "../state/video";
import type {
  VideoAsset,
  VideoFit,
  VjBlend,
  VjClip,
  VjGenerator,
  VjGeneratorKind,
  VjColumn,
  VjDeck,
  VjGrid,
  VjLayer,
  VjPlayback,
  VjSide,
  VjTransition,
  VjTrigger,
} from "../project/types";
import { useDoc } from "../state/store";
import { openMenu, sayError } from "../state/ui";
import type { MenuItem } from "../state/ui";
import type { VideoStrip } from "../video/api";
import {
  bindOutput,
  bindPlayState,
  edit,
  importVideos,
  launch,
  launchColumn,
  moveCrossfader,
  preview,
  release,
  selectClip,
  selectLayer,
  showBrowser,
  showMedia,
  showTab,
  stopLayer,
  useVj,
} from "./store";
import type { MediaKind } from "./store";
import { GENERATORS, swatch } from "./generators";
import { AdvancedOutput } from "./AdvancedOutput";
import "./vj.css";

/** One empty list for a project without videos, so a store selector answers the same value each time. */
const NO_VIDEOS: VideoAsset[] = [];

/** Whether a video can be shown yet: `failed` when the backend cannot read it, `processing` while it reads it. */
function useVideoState(asset: string | undefined): "failed" | "processing" | null {
  const error = useVideoErrors((errors) => (asset ? errors[asset] : undefined));
  const ready = useDoc((doc) => !asset || (asset in doc.video && asset in doc.thumbs));
  if (error) return "failed";
  return ready ? null : "processing";
}

/** The state of a clip's video over its picture: importing, processing or unreadable. */
function VideoState({ state, asset }: { state: "importing" | "processing" | "failed"; asset?: string }) {
  const t = useT();
  const error = useVideoErrors((errors) => (asset === undefined ? undefined : errors[asset]));
  return (
    <span className={`vj-state ${state}`} title={error}>
      {state !== "failed" && <i className="vj-spin" aria-hidden="true" />}
      {t(`vj.state.${state}`)}
    </span>
  );
}

const BLENDS: VjBlend[] = ["alpha", "add", "subtract", "multiply", "screen", "lighten", "darken"];
const TRANSITIONS: VjTransition[] = ["alpha", "add"];
const PLAYBACKS: VjPlayback[] = ["loop", "once", "pingPong"];
const MEDIA_KINDS: MediaKind[] = ["all", "video", "image"];
const TRIGGERS: VjTrigger[] = ["normal", "piano"];
const FITS: VideoFit[] = ["fit", "fill", "stretch", "none"];
/** The longest transition, in seconds, as the backend keeps it. */
const MAX_TRANSITION = 10;
const DRAG = "application/x-flux-vj";

type Dragged = { clip: string } | { asset: string } | { generator: VjGeneratorKind };

const fail = (err: unknown) => sayError(errorMessage(err));

/** `mm:ss` of a count of seconds. */
function clock(seconds: number): string {
  const whole = Math.max(0, Math.ceil(seconds));
  return `${String(Math.floor(whole / 60)).padStart(2, "0")}:${String(whole % 60).padStart(2, "0")}`;
}

const layerName = (t: Translate, grid: VjGrid, layer: VjLayer) =>
  layer.name ?? t("vj.layer.default", { n: grid.layers.indexOf(layer) + 1 });

function clipName(t: Translate, clip: VjClip, videos: VideoAsset[]): string {
  if (clip.name) return clip.name;
  if (clip.generator) return t(`vj.generator.${clip.generator.kind}`);
  return videos.find((video) => video.id === clip.asset)?.name ?? "";
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
function Fader({ letter, label, value, off, progress, onMove, onSet }: {
  letter: string;
  label: string;
  value: number;
  off?: boolean;
  /** How far what the fader times is, 0 to 1, while it runs: the transition of the T fader. */
  progress?: number | null;
  onMove: (value: number) => void;
  onSet: (value: number) => void;
}) {
  const [held, setHeld] = useState<number | null>(null);
  const shown = held ?? value;
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
      className={`vj-fader${off ? " off" : ""}`}
      data-letter={letter}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(shown * 100)}
      title={`${label} ${Math.round(shown * 100)}`}
      style={{ "--v": shown } as CSSProperties}
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
  const name = layerName(t, grid, layer);
  const clip = playingClip === undefined ? undefined : layer.clips?.find((c) => c.id === playingClip);
  const level = (op: "layerMaster" | "layerOpacity" | "layerVolume" | "layerTransition", scale = 1) => ({
    onMove: (value: number) => preview({ op, layer: layer.id, value: value * scale }),
    onSet: (value: number) => void edit({ op, layer: layer.id, value: value * scale }),
  });
  const side = (s: VjSide) => void edit({ op: "layerCrossfade", layer: layer.id, side: layer.crossfade === s ? null : s });
  const at = grid.layers.indexOf(layer);
  const menu: MenuItem[] = [
    { label: t("vj.layer.rename"), run: () => setRenaming(true) },
    { label: t("vj.layer.up"), disabled: at === grid.layers.length - 1, run: () => void edit({ op: "moveLayer", layer: layer.id, to: at + 1 }) },
    { label: t("vj.layer.down"), disabled: at === 0, run: () => void edit({ op: "moveLayer", layer: layer.id, to: at - 1 }) },
    "-",
    { label: t("vj.layer.remove"), disabled: grid.layers.length === 1, run: () => void edit({ op: "removeLayer", layer: layer.id }) },
  ];
  return (
    <div
      className={`vj-lh${playingClip !== undefined ? " playing" : ""}${layer.global ? " global" : ""}`}
      onClick={() => selectLayer(layer.id)}
      onContextMenu={(event) => {
        event.preventDefault();
        openMenu({ x: event.clientX, y: event.clientY, items: menu });
      }}
    >
      <button type="button" className="vj-eject" title={t("vj.layer.eject")} aria-label={t("vj.layer.eject")}
        onClick={(event) => { event.stopPropagation(); void stopLayer(layer.id); }}>
        <CloseIcon />
      </button>
      <div className="vj-bsb">
        <div className="vj-bs">
          <button type="button" className={layer.bypass ? "on bypass" : ""} aria-pressed={!!layer.bypass} title={t("vj.layer.bypass")}
            onClick={() => void edit({ op: "layerBypass", layer: layer.id, on: !layer.bypass })}>B</button>
          <button type="button" className={layer.solo ? "on solo" : ""} aria-pressed={!!layer.solo} title={t("vj.layer.solo")}
            onClick={() => void edit({ op: "layerSolo", layer: layer.id, on: !layer.solo })}>S</button>
        </div>
        <select className="vj-select" value={layer.blend ?? "alpha"} title={t("vj.layer.blend")} aria-label={t("vj.layer.blend")}
          onChange={(event) => void edit({ op: "layerBlend", layer: layer.id, blend: event.target.value as VjBlend })}>
          {BLENDS.map((blend) => <option key={blend} value={blend}>{t(`vj.blend.${blend}`)}</option>)}
        </select>
      </div>
      <div className="vj-faders">
        <Fader letter="M" label={t("vj.fader.master")} value={layer.master ?? 1} {...level("layerMaster")} />
        <Fader letter="A" label={t("vj.fader.audio")} value={layer.volume ?? 1} {...level("layerVolume")} />
        <Fader letter="V" label={t("vj.fader.video")} value={layer.opacity ?? 1} {...level("layerOpacity")} />
      </div>
      <Thumb className={`vj-now${clip ? "" : " empty"}`} strip={stripOf(thumbs, clip)} generator={clip?.generator}>
        {clip && <PlayTime layer={layer.id} countdown="header" />}
        {transition != null && <span className="vj-tbar"><i style={{ width: `${transition * 100}%` }} /></span>}
      </Thumb>
      <div className="vj-tcol">
        <Fader letter="T" label={t("vj.fader.transition")} value={(layer.transition ?? 0) / MAX_TRANSITION}
          progress={transition} {...level("layerTransition", MAX_TRANSITION)} />
        <select className="vj-select short" value={layer.transitionEffect ?? "alpha"} title={t("vj.layer.transitionEffect")}
          aria-label={t("vj.layer.transitionEffect")}
          onChange={(event) => void edit({ op: "layerTransitionEffect", layer: layer.id, effect: event.target.value as VjTransition })}>
          {TRANSITIONS.map((effect) => <option key={effect} value={effect}>{t(`vj.transition.short.${effect}`)}</option>)}
        </select>
      </div>
      <div className="vj-lname">
        <span className="vj-num">{at + 1}</span>
        {renaming ? (
          <InlineName value={layer.name ?? ""} label={t("vj.layer.rename")} onDone={() => setRenaming(false)}
            onCommit={(text) => void edit({ op: "renameLayer", layer: layer.id, name: text })} />
        ) : (
          <Marquee className="vj-nm" title={name} onDoubleClick={() => setRenaming(true)}>{name}</Marquee>
        )}
        {layer.global && <span className="vj-global" title={t("vj.layer.global")}><GlobeIcon /></span>}
        <Marquee className={`vj-clipname${clip ? "" : " idle"}`}>{clip ? clipName(t, clip, videos) : t("vj.layer.noClip")}</Marquee>
        <span className="vj-ab" role="group" aria-label={t("vj.layer.side")}>
          {(["a", "b"] as VjSide[]).map((s) => (
            <button key={s} type="button" className={layer.crossfade === s ? "on" : ""} aria-pressed={layer.crossfade === s}
              onClick={(event) => { event.stopPropagation(); side(s); }}>{s.toUpperCase()}</button>
          ))}
        </span>
      </div>
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

  const onDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setOver(false);
    const raw = event.dataTransfer.getData(DRAG);
    if (!raw) return;
    const dragged = JSON.parse(raw) as Dragged;
    if ("clip" in dragged) {
      if (dragged.clip !== clip?.id) void edit({ op: "moveClip", clip: dragged.clip, layer: layer.id, column: column.id });
    } else if ("generator" in dragged) {
      void edit({ op: "placeGenerator", generator: { kind: dragged.generator }, layer: layer.id, column: column.id });
    } else void edit({ op: "placeVideo", asset: dragged.asset, layer: layer.id, column: column.id });
  };
  const dropping = {
    onDragOver: (event: DragEvent<HTMLDivElement>) => {
      if (!event.dataTransfer.types.includes(DRAG)) return;
      event.preventDefault();
      setOver(true);
    },
    onDragLeave: () => setOver(false),
    onDrop,
  };

  if (!clip) {
    // As on a media server, an empty cell stops its layer, over the layer's transition.
    return (
      <div className={`vj-slot${over ? " over" : ""}`} title={t("vj.cell.empty")}
        onPointerDown={(event) => { if (event.button === 0) void stopLayer(layer.id); }} {...dropping} />
    );
  }
  const name = clipName(t, clip, videos);
  const piano = clip.trigger === "piano";
  return (
    <div
      className={`vj-clip${live ? " on-air" : ""}${selected ? " sel" : ""}${over ? " over" : ""}`}
      draggable
      onDragStart={(event) => event.dataTransfer.setData(DRAG, JSON.stringify({ clip: clip.id } satisfies Dragged))}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        void launch(clip.id, layer.id);
      }}
      onPointerUp={() => { if (piano) void release(clip.id); }}
      onPointerLeave={(event) => { if (piano && event.buttons === 1) void release(clip.id); }}
      onContextMenu={(event) => {
        event.preventDefault();
        selectClip(clip.id, layer.id);
        openMenu({ x: event.clientX, y: event.clientY, items: [
          { label: t("vj.clip.remove"), run: () => void edit({ op: "removeClip", clip: clip.id }) },
        ] });
      }}
      {...dropping}
    >
      <Thumb strip={stripOf(thumbs, clip)} generator={clip.generator} />
      <span className="vj-shade" />
      {live && <PlayTime layer={layer.id} clip={clip.id} countdown="cell" />}
      {state && <VideoState state={state} asset={clip.asset} />}
      {/* The name strip selects the clip without launching it. */}
      <div className="vj-pick" aria-label={t("vj.clip.select")}
        onPointerDown={(event) => { event.stopPropagation(); if (event.button === 0) selectClip(clip.id, layer.id); }}
        onPointerUp={(event) => event.stopPropagation()}>
        <Marquee className="vj-cn">{name}</Marquee>
        <span className="vj-cm">{t(`vj.trigger.short.${clip.trigger ?? "normal"}`)}{clip.playback && clip.playback !== "loop" ? ` · ${t(`vj.playback.short.${clip.playback}`)}` : ""}</span>
      </div>
    </div>
  );
}

function ColumnHead({ column, at, live }: { column: VjColumn; at: number; live: boolean }) {
  const t = useT();
  const [renaming, setRenaming] = useState(false);
  return (
    <button
      type="button"
      className={`vj-col${live ? " on-air" : ""}`}
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

function DeckPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const deck = grid.decks.find((d) => d.id === grid.deck) ?? grid.decks[0];
  const layers = [...grid.layers].reverse();
  const lit = useVj((vj) => vj.play.column);
  const liveColumn = deck.columns.findIndex((column) => column.id === lit);
  const sideNames = (side: VjSide) => grid.layers.filter((l) => l.crossfade === side).map((l) => layerName(t, grid, l)).join(", ");
  const columns = { "--columns": deck.columns.length } as CSSProperties;
  return (
    <section className="vj-panel vj-deck-panel" aria-label={t("vj.grid")}>
      <header className="vj-ph">
        <h2>{t("vj.grid")}</h2>
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
          {layers.map((layer, i) => (
            <div key={layer.id} className={`vj-row${layer.global ? " global" : ""}${i > 0 && layers[i - 1].global && !layer.global ? " split" : ""}`}>
              <LayerHeader grid={grid} layer={layer} />
              {deck.columns.map((column, at) => (
                <Cell key={column.id} layer={layer} column={column} at={at} />
              ))}
            </div>
          ))}
        </div>
        <div className="vj-decks" role="tablist" aria-label={t("vj.decks")}>
          {grid.decks.map((d, at) => <DeckTab key={d.id} deck={d} at={at} shown={d.id === deck.id} only={grid.decks.length === 1} />)}
          <button type="button" className="vj-deck add" title={t("vj.deck.add")} aria-label={t("vj.deck.add")}
            onClick={() => void edit({ op: "addDeck" })}><PlusIcon /></button>
        </div>
        <div className="vj-comp">
          <div className="vj-cl"><strong>{t("vj.composition")}</strong></div>
          <div className="vj-xf">
            <div className="vj-xs"><b>A</b><span>{sideNames("a")}</span></div>
            <Crossfader />
            <div className="vj-xs end"><span>{sideNames("b")}</span><b>B</b></div>
          </div>
          <button type="button" className="button" onClick={() => void stopLayer(null)}><StopIcon />{t("vj.stopAll")}</button>
        </div>
      </div>
    </section>
  );
}

/** What the screens show, as the backend draws it (see `bindOutput`). */
function OutputPanel() {
  const t = useT();
  const live = useVj((vj) => vj.play.playing.some((p) => p.level > 0));
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
    <section className="vj-panel" aria-label={t("vj.output")}>
      <header className="vj-ph">
        <h2>{t("vj.output")}</h2>
        <div className="vj-tools">{live && <span className="vj-pill"><i />{t("vj.onAir")}</span>}</div>
      </header>
      <div className="vj-pb vj-mon-wrap">
        <div className="vj-mon"><canvas ref={canvas} className="vj-out" /></div>
      </div>
    </section>
  );
}

function PreviewPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const selected = useVj((vj) => vj.clip);
  const thumbs = useDoc((doc) => doc.thumbs);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const found = grid.layers.flatMap((layer) => (layer.clips ?? []).map((clip) => ({ layer, clip }))).find((x) => x.clip.id === selected);
  const strip = stripOf(thumbs, found?.clip);
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    if (!strip || strip.count < 2) return;
    const timer = window.setInterval(() => setFrame((f) => (f + 1) % strip.count), Math.max(100, strip.seconds * 1000));
    return () => window.clearInterval(timer);
  }, [strip]);
  return (
    <section className="vj-panel" aria-label={t("vj.preview")}>
      <header className="vj-ph">
        <h2>{t("vj.preview")}</h2>
        <div className="vj-tools">{found && <Marquee className="vj-dim">{clipName(t, found.clip, videos)}</Marquee>}</div>
      </header>
      <div className="vj-pb vj-mon-wrap">
        <div className="vj-mon">
          {found ? <Thumb strip={strip} generator={found.clip.generator} position={strip ? (frame + 0.5) / strip.count : 0} /> : <p className="vj-empty">{t("vj.previewEmpty")}</p>}
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

/**
 * A number set by dragging its label sideways, as in Resolume, by typing it or with the arrow keys; a
 * double click on the label puts it back to `reset`. `value` and what it sends are in the backend's units, shown times
 * `shown` (e.g. 100 for a percent).
 */
function Scrub({ label, name, value, shown, reset, unit = "%", decimals = 0, onMove, onSet }: {
  /** The handle's short tag. */
  label: string;
  /** What the number is, for a screen reader. */
  name: string;
  value: number;
  shown: number;
  reset: number;
  /** Shown after the number. */
  unit?: string;
  /** Places after the point it is shown with. */
  decimals?: number;
  onMove: (value: number) => void;
  onSet: (value: number) => void;
}) {
  const t = useT();
  const [held, setHeld] = useState<{ x: number; from: number; v: number; moved: boolean } | null>(null);
  const [text, setText] = useState<string | null>(null);
  const current = held?.v ?? value;
  const display = (current * shown).toFixed(decimals);
  // One pixel is one shown unit; with Shift a tenth of one.
  const dragged = (event: PointerEvent<HTMLElement>, from: { x: number; from: number }) =>
    from.from + ((event.clientX - from.x) * (event.shiftKey ? 0.1 : 1)) / shown;
  const commit = () => {
    if (text !== null && text.trim() !== "" && Number.isFinite(Number(text))) onSet(Number(text) / shown);
    setText(null);
  };
  return (
    <span className="vj-scrub">
      <span
        className="vj-scrub-l"
        title={t("vj.clip.scrub")}
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId);
          setHeld({ x: event.clientX, from: value, v: value, moved: false });
        }}
        onPointerMove={(event) => {
          if (!held) return;
          const v = dragged(event, held);
          setHeld({ ...held, v, moved: true });
          onMove(v);
        }}
        onPointerUp={(event) => {
          if (!held) return;
          const v = dragged(event, held);
          setHeld(null);
          // Even back where it started: the previews moved the output, and this puts it back.
          if (held.moved) onSet(v);
        }}
        onPointerCancel={() => setHeld(null)}
        onDoubleClick={() => onSet(reset)}
      >{label}</span>
      <input className="vj-input vj-mono" inputMode="decimal" aria-label={name} value={text ?? display}
        onFocus={(event) => event.currentTarget.select()}
        onChange={(event) => setText(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
          if (event.key === "Escape") { setText(null); event.currentTarget.blur(); }
          // Arrows step one unit, ten with Shift.
          const step = event.key === "ArrowUp" ? 1 : event.key === "ArrowDown" ? -1 : 0;
          if (step) {
            event.preventDefault();
            setText(null);
            onSet(value + (step * (event.shiftKey ? 10 : 1)) / shown);
          }
        }} />
      <span className="vj-scrub-u">{unit}</span>
    </span>
  );
}

function Segments<T extends string>({ options, value, label, onPick }: {
  options: T[];
  value: T;
  label: (option: T) => string;
  onPick: (option: T) => void;
}) {
  return (
    <div className="vj-segs" role="radiogroup">
      {options.map((option) => (
        <button key={option} type="button" role="radio" aria-checked={option === value} className={option === value ? "on" : ""}
          onClick={() => { if (option !== value) onPick(option); }}>{label(option)}</button>
      ))}
    </div>
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

/** A colour picked in place: the output follows while it is picked, and it is kept when the picker closes. */
function ColourField({ value, label, onMove, onSet }: {
  value: string;
  label: string;
  onMove: (colour: string) => void;
  onSet: (colour: string) => void;
}) {
  const [colour, setColour] = useState(value);
  useEffect(() => setColour(value), [value]);
  return (
    <label className="vj-colour" title={label}>
      <input type="color" aria-label={label} value={colour}
        onChange={(event) => { setColour(event.target.value); onMove(event.target.value); }}
        onBlur={() => { if (colour !== value) onSet(colour); }} />
      <span className="vj-mono">{colour.toUpperCase()}</span>
    </label>
  );
}

/** The parameters of a generator clip, each with the backend's default where the clip has none. */
function GeneratorFields({ clip, generator }: { clip: string; generator: VjGenerator }) {
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
    <div className="vj-f" title={props.hint}><span>{props.name}</span>
      <div className="vj-scrubs one">
        <Scrub {...props} value={value} onMove={(v) => move({ [key]: v })} onSet={(v) => keep({ [key]: v })} />
      </div>
    </div>
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
    case "bars":
      return null;
  }
}

function Inspector({ grid }: { grid: VjGrid }) {
  const t = useT();
  // One value each, so the play state polled ten times a second does not draw the inspector again.
  const clipId = useVj((vj) => vj.clip);
  const layerId = useVj((vj) => vj.layer);
  const tab = useVj((vj) => vj.tab);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const probes = useDoc((doc) => doc.video);
  const thumbs = useDoc((doc) => doc.thumbs);
  const found = grid.layers.flatMap((layer) => (layer.clips ?? []).map((clip) => ({ layer, clip }))).find((x) => x.clip.id === clipId);
  const layer = grid.layers.find((l) => l.id === layerId) ?? found?.layer;
  const tabs = (
    <nav className="vj-tabs" role="tablist">
      {(["clip", "layer"] as const).map((key) => (
        <button key={key} type="button" role="tab" aria-selected={tab === key} className={tab === key ? "on" : ""} onClick={() => showTab(key)}>
          {t(`vj.inspector.${key}`)}
        </button>
      ))}
    </nav>
  );
  let body: ReactNode = <p className="vj-empty">{t(tab === "clip" ? "vj.inspector.noClip" : "vj.inspector.noLayer")}</p>;
  if (tab === "clip" && found) {
    const { clip } = found;
    const { generator } = clip;
    const video = videos.find((v) => v.id === clip.asset);
    const probe = clip.asset ? probes[clip.asset] : undefined;
    const deck = grid.decks.find((d) => d.id === grid.deck);
    const scale = clip.scale ?? 1;
    const [x, y] = clip.position ?? [0, 0];
    const columnAt = found.layer.global ? clip.slot ?? 0 : deck?.columns.findIndex((c) => c.id === clip.column) ?? -1;
    body = (
      <div className="vj-insp">
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
          {generator && <GeneratorFields clip={clip.id} generator={generator} />}
          {!generator && !probe?.still && (
            <div className="vj-f"><span>{t("vj.clip.playback")}</span>
              <Segments options={PLAYBACKS} value={clip.playback ?? "loop"} label={(o) => t(`vj.playback.${o}`)}
                onPick={(playback) => void edit({ op: "clipPlayback", clip: clip.id, playback })} />
            </div>
          )}
          {!generator && (
            <div className="vj-f"><span>{t("vj.clip.fit")}</span>
              <Segments options={FITS} value={clip.fit ?? "fit"} label={(o) => t(`vj.fit.${o}`)}
                onPick={(fit) => void edit({ op: "clipFit", clip: clip.id, fit })} />
            </div>
          )}
          <div className="vj-f"><span>{t("vj.clip.scale")}</span>
            <div className="vj-scrubs">
              <Scrub label="↔" name={t("vj.clip.scale")} value={scale} shown={100} reset={1}
                onMove={(value) => preview({ op: "clipScale", clip: clip.id, value })}
                onSet={(value) => void edit({ op: "clipScale", clip: clip.id, value })} />
            </div>
          </div>
          <div className="vj-f"><span>{t("vj.clip.position")}</span>
            <div className="vj-scrubs">
              <Scrub label="X" name={t("vj.clip.x")} value={x} shown={100} reset={0}
                onMove={(v) => preview({ op: "clipPosition", clip: clip.id, x: v, y })}
                onSet={(v) => void edit({ op: "clipPosition", clip: clip.id, x: v, y })} />
              <Scrub label="Y" name={t("vj.clip.y")} value={y} shown={100} reset={0}
                onMove={(v) => preview({ op: "clipPosition", clip: clip.id, x, y: v })}
                onSet={(v) => void edit({ op: "clipPosition", clip: clip.id, x, y: v })} />
            </div>
          </div>
        </div>
        <dl className="vj-meta">
          {generator ? (
            <div><dt>{t("vj.gen.kind")}</dt><dd>{t(`vj.generator.${generator.kind}`)}</dd></div>
          ) : (
            <>
              <div><dt>{t("vj.clip.file")}</dt><dd><Marquee>{video?.name ?? ""}</Marquee></dd></div>
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
            <NameField value={layer.name ?? ""} placeholder={layerName(t, grid, { ...layer, name: undefined })} label={t("vj.layer.name")}
              onCommit={(name) => edit({ op: "renameLayer", layer: layer.id, name })} />
          </label>
          <label className="vj-f"><span>{t("vj.layer.blend")}</span>
            <select className="vj-select wide" value={layer.blend ?? "alpha"}
              onChange={(event) => void edit({ op: "layerBlend", layer: layer.id, blend: event.target.value as VjBlend })}>
              {BLENDS.map((blend) => <option key={blend} value={blend}>{t(`vj.blend.${blend}`)}</option>)}
            </select>
          </label>
          <div className="vj-f"><span>{t("vj.layer.transitionEffect")}</span>
            <Segments options={TRANSITIONS} value={layer.transitionEffect ?? "alpha"} label={(o) => t(`vj.transition.${o}`)}
              onPick={(effect) => void edit({ op: "layerTransitionEffect", layer: layer.id, effect })} />
          </div>
          <div className="vj-f"><span>{t("vj.layer.transition")}</span>
            <span className="vj-mono">{t("vj.seconds", { n: (layer.transition ?? 0).toFixed(1) })}</span>
          </div>
          <div className="vj-f"><span>{t("vj.layer.kind")}</span>
            <span>{t(layer.global ? "vj.layer.kindGlobal" : "vj.layer.kindDeck")}</span>
          </div>
        </div>
      </div>
    );
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
          {(["media", "generators"] as const).map((key) => (
            <button key={key} type="button" role="tab" aria-selected={browser === key} className={browser === key ? "on" : ""}
              onClick={() => showBrowser(key)}>{t(key === "media" ? "vj.media" : "vj.browser.generators")}</button>
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
      </header>
      <div className="vj-pb">
        {browser === "generators" ? (
          <div className="vj-mg">
            {GENERATORS.map((kind) => (
              <figure key={kind} className="vj-m" draggable title={t("vj.generators.note")}
                onDragStart={(event) => event.dataTransfer.setData(DRAG, JSON.stringify({ generator: kind } satisfies Dragged))}>
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
                <figure key={video.id} className={`vj-m${onAir.has(video.id) ? " using" : ""}`} draggable
                  onDragStart={(event) => event.dataTransfer.setData(DRAG, JSON.stringify({ asset: video.id } satisfies Dragged))}>
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
  if (!grid) return null;
  return (
    <div className="vj-app">
      <DeckPanel grid={grid} />
      <div className="vj-right">
        <OutputPanel />
        <PreviewPanel grid={grid} />
      </div>
      <Inspector grid={grid} />
      <MediaPanel grid={grid} />
      <AdvancedOutput />
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
