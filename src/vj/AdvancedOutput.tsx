// The Advanced Output of a VJ show, as on a media server: the screens with their slices and masks on
// the left, the composition (Input selection) or one screen (Output transformation) in the middle,
// and what is selected on the right. The backend keeps the mapping and every number in bounds; this
// shows the live composition and a drag until it is let go.

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { useT } from "../i18n";
import { SelectField, TextField } from "../components/fields";
import { CloseIcon, EyeIcon, EyeOffIcon, MaskIcon, PlusIcon, ScreenIcon, SliceIcon } from "../components/icons";
import { useDoc } from "../state/store";
import { DEFAULT_SIZE, MaskFields, SliceFields, drag } from "../setup/MappingEditor";
import { OFF, displayLabel, screenNote, useOutputStatus } from "../setup/VideoPage";
import {
  MAIN_SCREEN,
  compositionSet,
  maskEdit,
  mappingEdit,
  screenEdit,
  showTestPattern,
  useComposition,
} from "../video/mapping";
import { ScreenSwitch } from "../video/ScreenSwitch";
import type { Mask, Point, Size, Slice, VideoScreen } from "../video/mapping";
import { TestCardFields } from "./TestCardFields";
import { closeOutput } from "./menus";
import { bindOutput, useVj } from "./store";
import { picture } from "./picture";

type Item = { kind: "screen" } | { kind: "slice"; id: string } | { kind: "mask"; id: string };
type Stage = "input" | "output";

const NO_SCREENS: VideoScreen[] = [];
const NO_SLICES: Slice[] = [];
const NO_MASKS: Mask[] = [];
const pct = (v: number) => `${v * 100}%`;
/** The width the screen's preview is drawn at; its height follows the display's shape. */
const PREVIEW_WIDTH = 1280;
/** Each slice is drawn in this many strips each way, so its perspective bends as the output's does. */
const STEPS = 8;

/** Opens with the Output menu; closes with its button or Escape. */
export function AdvancedOutput() {
  const open = useVj((vj) => vj.output);
  return open ? <OutputDialog /> : null;
}

/**
 * The latest picture of the composition, kept in a canvas for the stages to draw from, and a way to
 * hear when it changes without the whole window drawing again.
 */
function useLivePicture(screenId: string) {
  const source = useMemo(() => document.createElement("canvas"), []);
  // The test cards the backend lays over the picture, small and clear where there is none.
  const card = useMemo(() => document.createElement("canvas"), []);
  const listeners = useRef(new Set<() => void>());
  const paint = () => {
    const context = source.getContext("2d");
    if (context && card.width > 0 && source.width > 0) context.drawImage(card, 0, 0, source.width, source.height);
  };
  useEffect(
    () =>
      bindOutput((picture) => {
        const context = source.getContext("2d");
        if (!context) return;
        if (picture) {
          if (source.width !== picture.width || source.height !== picture.height) {
            source.width = picture.width;
            source.height = picture.height;
          }
          context.putImageData(picture, 0, 0);
        } else context.clearRect(0, 0, source.width, source.height);
        paint();
        listeners.current.forEach((draw) => draw());
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [source, card],
  );
  useEffect(() => {
    let stopped = false;
    const ask = () =>
      void invoke<ArrayBuffer>("video_card_preview", { screen: screenId }).then(
        (bytes) => {
          if (stopped) return;
          const image = picture(bytes);
          if (!image) card.width = 0;
          else {
            card.width = image.width;
            card.height = image.height;
            card.getContext("2d")?.putImageData(image, 0, 0);
            if (source.width === 0) {
              source.width = image.width;
              source.height = image.height;
            }
          }
          paint();
          listeners.current.forEach((draw) => draw());
        },
        () => undefined,
      );
    ask();
    const timer = window.setInterval(ask, 700);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [screenId, card, source]);
  const subscribe = (draw: () => void) => {
    listeners.current.add(draw);
    return () => void listeners.current.delete(draw);
  };
  return { source, subscribe };
}
type Live = ReturnType<typeof useLivePicture>;

function OutputDialog() {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);
  const { status, choose } = useOutputStatus();
  const screens = useDoc((doc) => doc.project.output.screens ?? NO_SCREENS);
  const composition = useComposition();
  const [stage, setStage] = useState<Stage>("input");
  const [screenId, setScreenId] = useState(MAIN_SCREEN);
  const live = useLivePicture(screenId);
  const [item, setItem] = useState<Item>({ kind: "screen" });
  const [point, setPoint] = useState(0);
  const [pattern, setPattern] = useState(false);

  useEffect(() => {
    ref.current?.showModal();
    return () => void showTestPattern(false);
  }, []);

  const listed = status?.screens ?? [];
  const current = listed.find((s) => s.id === screenId) ?? listed[0];
  const screen = screens.find((s) => s.id === current?.id);
  const slices = screen?.slices ?? NO_SLICES;
  const masks = screen?.masks ?? NO_MASKS;
  const slice = item.kind === "slice" ? slices.find((s) => s.id === item.id) : undefined;
  const mask = item.kind === "mask" ? masks.find((m) => m.id === item.id) : undefined;
  const chosen = current?.chosen ?? OFF;
  const display: Size = status?.displays.find((d) => d.id === chosen) ?? DEFAULT_SIZE;
  const picture: Size = { width: composition.width, height: composition.height };

  const select = (screen: string, next: Item) => {
    setScreenId(screen);
    setItem(next);
    if (next.kind === "mask") setStage("output");
  };
  /** The last slice or mask of `screen` once an add has landed, selected. */
  const selectAdded = (screen: string, kind: "slice" | "mask") => {
    const found = useDoc.getState().project.output.screens?.find((s) => s.id === screen);
    const list = kind === "slice" ? found?.slices : found?.masks;
    const added = list?.[list.length - 1];
    if (added) select(screen, { kind, id: added.id });
  };

  return (
    <dialog ref={ref} className="vj-ao" aria-labelledby="vj-ao-title" onCancel={closeOutput}>
      <header className="vj-ao-head">
        <h2 id="vj-ao-title">{t("vj.ao.title")}</h2>
        <div className="vj-segs" role="tablist" aria-label={t("vj.ao.stage")}>
          {(["input", "output"] as const).map((id) => (
            <button key={id} type="button" role="tab" aria-selected={stage === id} className={stage === id ? "on" : undefined} onClick={() => setStage(id)}>
              {t(id === "input" ? "vj.ao.input" : "vj.ao.output")}
            </button>
          ))}
        </div>
        <div className="vj-ao-comp">
          <span>{t("setup.video.composition")}</span>
          <TextField
            label={t("setup.video.field.width")}
            inputMode="numeric"
            monospace
            value={String(composition.width)}
            onCommit={(width) => void compositionSet(width, String(composition.height))}
          />
          <span aria-hidden="true">×</span>
          <TextField
            label={t("setup.video.field.height")}
            inputMode="numeric"
            monospace
            value={String(composition.height)}
            onCommit={(height) => void compositionSet(String(composition.width), height)}
          />
        </div>
        <button
          type="button"
          className="button"
          aria-pressed={pattern}
          title={t("vj.ao.patternNote")}
          onClick={() => {
            setPattern(!pattern);
            void showTestPattern(!pattern);
          }}
        >
          {t("setup.video.pattern")}
        </button>
        <button type="button" className="icon-button" aria-label={t("setup.close")} title={t("setup.close")} onClick={closeOutput}>
          <CloseIcon />
        </button>
      </header>
      <div className="vj-ao-body">
        <nav className="vj-ao-tree" aria-label={t("setup.video.screens")}>
          <header className="vj-ao-ph">
            <h3>{t("setup.video.screens")}</h3>
            <button
              type="button"
              className="vj-tb"
              onClick={() =>
                void screenEdit({ op: "add" }).then(() => {
                  const list = useDoc.getState().project.output.screens ?? [];
                  const added = list[list.length - 1];
                  if (added) select(added.id, { kind: "screen" });
                })
              }
            >
              <PlusIcon />
              {t("vj.ao.screen")}
            </button>
          </header>
          {!status && <p className="vj-empty">{t("setup.video.loading")}</p>}
          <ul className="vj-ao-screens">
            {listed.map((s) => {
              const own = screens.find((x) => x.id === s.id);
              const on = s.id === current?.id;
              return (
                <li key={s.id} data-off={own?.enabled === false}>
                  <div className="vj-ao-item vj-ao-head">
                    <ScreenSwitch id={s.id} />
                    <button
                      type="button"
                      className="vj-ao-row vj-ao-screen"
                      aria-current={on && item.kind === "screen" ? "true" : undefined}
                      data-open={s.open}
                      onClick={() => select(s.id, { kind: "screen" })}
                    >
                      <ScreenIcon />
                      <span className="vj-ao-name">{s.name}</span>
                      <span className="screen-dot" title={screenNote(t, s)} />
                    </button>
                  </div>
                  <ul>
                    {(own?.slices ?? NO_SLICES).map((x) => (
                      <TreeRow
                        key={x.id}
                        icon={<SliceIcon />}
                        name={x.name}
                        hidden={x.hidden ?? false}
                        selected={on && slice?.id === x.id}
                        onSelect={() => select(s.id, { kind: "slice", id: x.id })}
                        onHide={() => void mappingEdit(s.id, { op: "setHidden", id: x.id, hidden: !x.hidden })}
                      />
                    ))}
                    {(own?.masks ?? NO_MASKS).map((x) => (
                      <TreeRow
                        key={x.id}
                        icon={<MaskIcon />}
                        name={x.name}
                        hidden={x.hidden ?? false}
                        selected={on && mask?.id === x.id}
                        onSelect={() => select(s.id, { kind: "mask", id: x.id })}
                        onHide={() => void maskEdit(s.id, { op: "setHidden", id: x.id, hidden: !x.hidden })}
                      />
                    ))}
                  </ul>
                  {on && (
                    <div className="vj-ao-adds">
                      <button type="button" className="vj-tb" onClick={() => void mappingEdit(s.id, { op: "add" }).then(() => selectAdded(s.id, "slice"))}>
                        <PlusIcon />
                        {t("vj.ao.slice")}
                      </button>
                      <button type="button" className="vj-tb" onClick={() => void maskEdit(s.id, { op: "add" }).then(() => selectAdded(s.id, "mask"))}>
                        <PlusIcon />
                        {t("vj.ao.mask")}
                      </button>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </nav>
        <section className="vj-ao-stage" aria-label={t(stage === "input" ? "vj.ao.input" : "vj.ao.output")}>
          {current && (
            <Stages
              key={`${current.id}-${stage}`}
              stage={stage}
              screenId={current.id}
              slices={slices}
              masks={masks}
              picture={picture}
              display={display}
              live={live}
              slice={slice}
              mask={mask}
              point={point}
              onSelect={(next) => select(current.id, next)}
              onPoint={setPoint}
            />
          )}
          <p className="vj-ao-hint">
            {stage === "input"
              ? t(slices.length ? "vj.ao.inputHint" : "vj.ao.noSlices")
              : t("vj.ao.outputHint", { width: display.width, height: display.height })}
          </p>
        </section>
        <aside className="vj-ao-props" aria-label={t("vj.ao.properties")}>
          {current && slice && (
            <>
              <SliceFields screenId={current.id} slice={slice} picture={picture} display={display} />
              <TestCardFields screenId={current.id} slice={slice} index={Math.max(0, slices.filter((s) => !s.hidden).findIndex((s) => s.id === slice.id))} />
            </>
          )}
          {current && mask && (
            <MaskFields screenId={current.id} mask={mask} point={point} setPoint={setPoint} onRemoved={() => setItem({ kind: "screen" })} />
          )}
          {current && !slice && !mask && (
            <div className="mapping-slice">
              <TextField label={t("setup.video.screenName")} value={current.name} onCommit={(name) => void screenEdit({ op: "rename", id: current.id, name })} />
              <div className="field">
                <label htmlFor="screen-output">{t("vj.ao.screenOutput")}</label>
                <ScreenSwitch id={current.id} switchId="screen-output" />
              </div>
              <SelectField label={t("setup.video.display")} value={chosen} hint={t("setup.video.mainNote")} onChange={(id) => void choose(current.id, id)}>
                <option value={OFF}>{t("setup.video.off")}</option>
                {status?.displays.map((d, index) => (
                  <option key={d.id} value={d.id}>
                    {displayLabel(t, d, index)}
                  </option>
                ))}
                {current.missing && <option value={chosen}>{t("setup.video.disconnected")}</option>}
              </SelectField>
              <p className="field-note" role="status">
                {screenNote(t, current)}
              </p>
              <dl className="vj-meta">
                <div>
                  <dt>{t("vj.ao.slices")}</dt>
                  <dd className="vj-num">{slices.length}</dd>
                </div>
                <div>
                  <dt>{t("vj.ao.masks")}</dt>
                  <dd className="vj-num">{masks.length}</dd>
                </div>
              </dl>
              {status?.decoding && (
                <p className="field-note">
                  {status.decoding === "cpu" ? t("setup.video.decodingCpu") : t("setup.video.decodingGpu", { device: status.decoding })}
                </p>
              )}
              {current.id !== MAIN_SCREEN && (
                <div className="button-row">
                  <button
                    type="button"
                    className="button danger"
                    onClick={() => {
                      select(MAIN_SCREEN, { kind: "screen" });
                      void screenEdit({ op: "remove", id: current.id });
                    }}
                  >
                    {t("setup.video.removeScreen")}
                  </button>
                </div>
              )}
            </div>
          )}
        </aside>
      </div>
    </dialog>
  );
}

function TreeRow({
  icon,
  name,
  hidden,
  selected,
  onSelect,
  onHide,
}: {
  icon: ReactNode;
  name: string;
  hidden: boolean;
  selected: boolean;
  onSelect: () => void;
  onHide: () => void;
}) {
  const t = useT();
  return (
    <li className="vj-ao-item" data-hidden={hidden}>
      <button type="button" className="vj-ao-row" aria-current={selected ? "true" : undefined} onClick={onSelect}>
        {icon}
        <span className="vj-ao-name">{name}</span>
      </button>
      <button
        type="button"
        className="vj-ao-eye"
        aria-pressed={!hidden}
        aria-label={t(hidden ? "vj.ao.show" : "setup.video.hide")}
        title={t(hidden ? "vj.ao.show" : "setup.video.hide")}
        onClick={onHide}
      >
        {hidden ? <EyeOffIcon /> : <EyeIcon />}
      </button>
    </li>
  );
}

/** Where on `screen` the point (`u`, `v`) of a slice lands, 0 to 1 across it, in perspective. */
function project(corners: Slice["corners"], u: number, v: number): Point {
  const [p0, p1, p2, p3] = corners;
  const dx1 = p1.x - p2.x, dx2 = p3.x - p2.x, dx3 = p0.x - p1.x + p2.x - p3.x;
  const dy1 = p1.y - p2.y, dy2 = p3.y - p2.y, dy3 = p0.y - p1.y + p2.y - p3.y;
  const det = dx1 * dy2 - dx2 * dy1;
  const g = det === 0 ? 0 : (dx3 * dy2 - dx2 * dy3) / det;
  const h = det === 0 ? 0 : (dx1 * dy3 - dx3 * dy1) / det;
  const w = g * u + h * v + 1;
  const x = (p1.x - p0.x + g * p1.x) * u + (p3.x - p0.x + h * p3.x) * v + p0.x;
  const y = (p1.y - p0.y + g * p1.y) * u + (p3.y - p0.y + h * p3.y) * v + p0.y;
  return { x: x / w, y: y / w };
}

/** Draws the triangle `s` of `image` onto the triangle `d` of `context`. */
function triangle(context: CanvasRenderingContext2D, image: CanvasImageSource, s: Point[], d: Point[]) {
  const [s0, s1, s2] = s;
  const [d0, d1, d2] = d;
  const u1x = s1.x - s0.x, u1y = s1.y - s0.y, u2x = s2.x - s0.x, u2y = s2.y - s0.y;
  const det = u1x * u2y - u2x * u1y;
  if (det === 0) return;
  const v1x = d1.x - d0.x, v1y = d1.y - d0.y, v2x = d2.x - d0.x, v2y = d2.y - d0.y;
  const a = (v1x * u2y - v2x * u1y) / det;
  const c = (v2x * u1x - v1x * u2x) / det;
  const b = (v1y * u2y - v2y * u1y) / det;
  const e = (v2y * u1x - v1y * u2x) / det;
  context.save();
  context.beginPath();
  // A hair larger than the triangle, so the seams between strips do not show.
  const mid = { x: (d0.x + d1.x + d2.x) / 3, y: (d0.y + d1.y + d2.y) / 3 };
  [d0, d1, d2].forEach((p, i) => {
    const len = Math.hypot(p.x - mid.x, p.y - mid.y) || 1;
    const q = { x: p.x + ((p.x - mid.x) / len) * 1, y: p.y + ((p.y - mid.y) / len) * 1 };
    if (i === 0) context.moveTo(q.x, q.y);
    else context.lineTo(q.x, q.y);
  });
  context.closePath();
  context.clip();
  context.setTransform(a, b, c, e, d0.x - a * s0.x - c * s0.y, d0.y - b * s0.x - e * s0.y);
  context.drawImage(image, 0, 0);
  context.restore();
}

/**
 * What one screen shows, drawn from the live composition: each slice cut out and placed on its four
 * corners, the masks over them in black. Soft edges are left to the real output.
 */
function drawScreen(context: CanvasRenderingContext2D, source: HTMLCanvasElement, slices: Slice[], masks: Mask[]) {
  const { width, height } = context.canvas;
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.fillStyle = "#000";
  context.fillRect(0, 0, width, height);
  if (source.width === 0 || source.height === 0) return;
  if (slices.length === 0) {
    // No slices: the whole picture, fitted to the screen.
    const scale = Math.min(width / source.width, height / source.height);
    const w = source.width * scale, h = source.height * scale;
    context.drawImage(source, (width - w) / 2, (height - h) / 2, w, h);
  }
  for (const slice of slices) {
    if (slice.hidden) continue;
    const r = slice.input;
    const from = (u: number, v: number) => ({ x: (r.x + r.width * u) * source.width, y: (r.y + r.height * v) * source.height });
    const to = (u: number, v: number) => {
      const p = project(slice.corners, u, v);
      return { x: p.x * width, y: p.y * height };
    };
    for (let i = 0; i < STEPS; i++) {
      for (let j = 0; j < STEPS; j++) {
        const u0 = i / STEPS, u1 = (i + 1) / STEPS, v0 = j / STEPS, v1 = (j + 1) / STEPS;
        triangle(context, source, [from(u0, v0), from(u1, v0), from(u1, v1)], [to(u0, v0), to(u1, v0), to(u1, v1)]);
        triangle(context, source, [from(u0, v0), from(u1, v1), from(u0, v1)], [to(u0, v0), to(u1, v1), to(u0, v1)]);
      }
    }
  }
  context.fillStyle = "#000";
  for (const mask of masks) {
    if (mask.hidden || mask.points.length < 3) continue;
    context.beginPath();
    if (mask.invert) context.rect(0, 0, width, height);
    mask.points.forEach((p, i) => (i === 0 ? context.moveTo(p.x * width, p.y * height) : context.lineTo(p.x * width, p.y * height)));
    context.closePath();
    context.fill("evenodd");
  }
}

function Stages({
  stage,
  screenId,
  slices,
  masks,
  picture,
  display,
  live,
  slice,
  mask,
  point,
  onSelect,
  onPoint,
}: {
  stage: Stage;
  screenId: string;
  slices: Slice[];
  masks: Mask[];
  picture: Size;
  display: Size;
  live: Live;
  slice: Slice | undefined;
  mask: Mask | undefined;
  point: number;
  onSelect: (item: Item) => void;
  onPoint: (point: number) => void;
}) {
  const t = useT();
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  // The slice or mask being dragged, as it looks before the backend has it.
  const [draft, setDraft] = useState<Slice | null>(null);
  const [maskDraft, setMaskDraft] = useState<Mask | null>(null);
  const shown = (s: Slice) => (draft?.id === s.id ? draft : s);
  const shownMask = (m: Mask) => (maskDraft?.id === m.id ? maskDraft : m);
  const drawn = useRef({ slices, masks });
  drawn.current = { slices: slices.map(shown), masks: masks.map(shownMask) };
  const size = stage === "input" ? picture : display;

  // Drawn on every picture from the backend, and at once when a drag or an edit moves something.
  const draw = () => {
    const target = canvas.current;
    const context = target?.getContext("2d");
    if (!target || !context) return;
    if (stage === "input") {
      if (target.width !== live.source.width || target.height !== live.source.height) {
        target.width = live.source.width || 1;
        target.height = live.source.height || 1;
      }
      context.clearRect(0, 0, target.width, target.height);
      context.drawImage(live.source, 0, 0);
    } else {
      const height = Math.round((PREVIEW_WIDTH * display.height) / display.width);
      if (target.width !== PREVIEW_WIDTH || target.height !== height) {
        target.width = PREVIEW_WIDTH;
        target.height = height;
      }
      drawScreen(context, live.source, drawn.current.slices, drawn.current.masks);
    }
  };
  const drawRef = useRef(draw);
  drawRef.current = draw;
  useEffect(() => live.subscribe(() => drawRef.current()), [live]);
  useEffect(() => drawRef.current());

  const commit = (s: Slice, next: Slice, op: "setInput" | "setCorners") => {
    if (next === s) return setDraft(null);
    const sent =
      op === "setInput"
        ? mappingEdit(screenId, { op, id: s.id, input: next.input })
        : mappingEdit(screenId, { op, id: s.id, corners: next.corners });
    void sent.finally(() => setDraft(null));
  };

  const moveRect = (s: Slice, event: ReactPointerEvent<Element>, resize: boolean) => {
    onSelect({ kind: "slice", id: s.id });
    let next = s;
    drag(
      event,
      box.current,
      (d) => {
        const r = s.input;
        const input = resize
          ? { ...r, width: Math.max(0.005, Math.min(1 - r.x, r.width + d.x)), height: Math.max(0.005, Math.min(1 - r.y, r.height + d.y)) }
          : { ...r, x: Math.max(0, Math.min(1 - r.width, r.x + d.x)), y: Math.max(0, Math.min(1 - r.height, r.y + d.y)) };
        next = { ...s, input };
        setDraft(next);
      },
      () => commit(s, next, "setInput"),
    );
  };

  /** Drags one corner of a slice, or all four when `index` is null. */
  const moveCorners = (s: Slice, index: number | null, event: ReactPointerEvent<Element>) => {
    onSelect({ kind: "slice", id: s.id });
    let next = s;
    drag(
      event,
      box.current,
      (d) => {
        const corners = s.corners.map((p, i) => (index === null || i === index ? { x: p.x + d.x, y: p.y + d.y } : p)) as Slice["corners"];
        next = { ...s, corners };
        setDraft(next);
      },
      () => commit(s, next, "setCorners"),
    );
  };

  /** Drags one point of a mask, or the whole mask when `index` is null. */
  const moveMask = (m: Mask, index: number | null, event: ReactPointerEvent<Element>) => {
    onSelect({ kind: "mask", id: m.id });
    if (index !== null) onPoint(index);
    let next = m;
    drag(
      event,
      box.current,
      (d) => {
        next = { ...m, points: m.points.map((p, i) => (index === null || i === index ? { x: p.x + d.x, y: p.y + d.y } : p)) };
        setMaskDraft(next);
      },
      () => {
        if (next === m) return setMaskDraft(null);
        void maskEdit(screenId, { op: "setPoints", id: m.id, points: next.points }).finally(() => setMaskDraft(null));
      },
    );
  };

  return (
    <div className="vj-ao-fit">
      <div className="vj-ao-canvas" ref={box} style={{ aspectRatio: `${size.width} / ${size.height}`, "--ar": size.width / size.height } as CSSProperties}>
        <canvas ref={canvas} className="vj-ao-picture" />
        {stage === "input" &&
          slices.map((s) => {
            const r = shown(s).input;
            const on = s.id === slice?.id;
            return (
              <div
                key={s.id}
                className="mapping-rect"
                data-selected={on}
                data-hidden={s.hidden ?? false}
                style={{ left: pct(r.x), top: pct(r.y), width: pct(r.width), height: pct(r.height) }}
                onPointerDown={(e) => moveRect(s, e, false)}
              >
                <span className="mapping-label">{s.name}</span>
                {on && (
                  <span
                    className="mapping-handle mapping-resize"
                    role="presentation"
                    title={t("setup.video.resize", { name: s.name })}
                    onPointerDown={(e) => moveRect(s, e, true)}
                  />
                )}
              </div>
            );
          })}
        {stage === "output" && (
          <>
            <svg className="mapping-quads" viewBox="0 0 1 1" preserveAspectRatio="none" aria-hidden="true">
              {slices.map((s) => (
                <polygon
                  key={s.id}
                  points={shown(s).corners.map((p) => `${p.x},${p.y}`).join(" ")}
                  data-selected={s.id === slice?.id}
                  data-hidden={s.hidden ?? false}
                  vectorEffect="non-scaling-stroke"
                  onPointerDown={(e) => moveCorners(s, null, e)}
                />
              ))}
              {masks.map((m) => (
                <polygon
                  key={m.id}
                  className="mapping-mask"
                  points={shownMask(m).points.map((p) => `${p.x},${p.y}`).join(" ")}
                  data-selected={m.id === mask?.id}
                  data-hidden={m.hidden ?? false}
                  data-invert={m.invert ?? false}
                  vectorEffect="non-scaling-stroke"
                  onPointerDown={(e) => moveMask(m, null, e)}
                />
              ))}
            </svg>
            {mask &&
              shownMask(mask).points.map((p, i) => (
                <span
                  key={i}
                  className="mapping-handle mapping-point"
                  data-selected={i === point}
                  role="presentation"
                  title={t("setup.video.maskPoint", { name: mask.name, n: i + 1 })}
                  style={{ left: pct(p.x), top: pct(p.y) }}
                  onPointerDown={(e) => moveMask(mask, i, e)}
                />
              ))}
            {slice &&
              shown(slice).corners.map((p, i) => (
                <span
                  key={i}
                  className="mapping-handle mapping-corner"
                  data-corner={i}
                  role="presentation"
                  title={t("setup.video.corner", { name: slice.name, n: i + 1 })}
                  style={{ left: pct(p.x), top: pct(p.y) }}
                  onPointerDown={(e) => moveCorners(slice, i, e)}
                />
              ))}
          </>
        )}
      </div>
    </div>
  );
}
