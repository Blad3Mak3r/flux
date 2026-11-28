// The two stages of the Advanced Output: the input picture with its slices and masks, and the output screen they are placed on.

import { useEffect, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent } from "react";
import { useT } from "../i18n";
import { drag } from "../setup/MappingEditor";
import { maskEdit, mappingEdit } from "../video/mapping";
import type { Mask, Size, Slice } from "../video/mapping";
import { drawScreen } from "./OutputPreview";
import type { Live } from "./OutputPreview";

export type Item = { kind: "screen" } | { kind: "slice"; id: string } | { kind: "mask"; id: string };

export type Stage = "input" | "output";

const pct = (v: number) => `${v * 100}%`;

/** The width the screen's preview is drawn at; its height follows the display's shape. */
const PREVIEW_WIDTH = 1280;

export function Stages({
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
