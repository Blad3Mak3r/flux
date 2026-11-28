import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { useT } from "../i18n";
import { useDoc } from "../state/store";
import { maskEdit, mappingEdit, showTestPattern, useComposition } from "../video/mapping";
import type { Mask, MappingOp, MaskOp } from "../video/mapping";
import type { Point, Rect, Size, Slice } from "../video/mapping";
import { MaskFields, SliceFields } from "./SliceFields";

const NO_SLICES: Slice[] = [];

/** Where the pointer is over `box`, as 0 to 1 of its width and height. */
export function at(
  event: { clientX: number; clientY: number },
  box: HTMLElement,
): Point {
  const r = box.getBoundingClientRect();
  return {
    x: (event.clientX - r.left) / r.width,
    y: (event.clientY - r.top) / r.height,
  };
}

/**
 * Follows a drag on `box` and calls `onMove` with how far the pointer went (as 0 to 1 of the box)
 * and `onEnd` once on release. Only the release reaches the backend, so one drag is one undo step.
 */
export function drag(
  event: ReactPointerEvent<Element>,
  box: HTMLElement | null,
  onMove: (delta: Point) => void,
  onEnd: () => void,
) {
  if (!box || event.button !== 0) return;
  event.preventDefault();
  event.stopPropagation();
  // An SVG shape takes the same pointer events as an HTML element.
  const handle = event.currentTarget as HTMLElement;
  handle.setPointerCapture(event.pointerId);
  const from = at(event, box);
  const move = (e: PointerEvent) => {
    const now = at(e, box);
    onMove({ x: now.x - from.x, y: now.y - from.y });
  };
  const up = () => {
    handle.removeEventListener("pointermove", move);
    handle.removeEventListener("pointerup", up);
    handle.removeEventListener("pointercancel", up);
    onEnd();
  };
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", up);
  handle.addEventListener("pointercancel", up);
}

const pct = (v: number) => `${v * 100}%`;

export const DEFAULT_SIZE: Size = { width: 1920, height: 1080 };

const NO_MASKS: Mask[] = [];

/**
 * The slices of one screen of the video output: on the left the picture, where each slice is a rectangle to
 * move or resize; on the right the screen, where the selected slice's four corners are dragged.
 * The backend keeps every number within bounds; this only shows a drag until it is let go.
 */
export function MappingEditor({
  screenId,
  screen,
}: {
  /** The screen of the project whose slices are edited. */
  screenId: string;
  /** The size of the display it is on, when it has one. */
  screen: { width: number; height: number } | null;
}) {
  const t = useT();
  const slices = useDoc(
    (doc) =>
      doc.project.output.screens?.find((s) => s.id === screenId)?.slices ??
      NO_SLICES,
  );
  const masks = useDoc(
    (doc) =>
      doc.project.output.screens?.find((s) => s.id === screenId)?.masks ??
      NO_MASKS,
  );
  const edit = (op: MappingOp) => mappingEdit(screenId, op);
  const editMask = (op: MaskOp) => maskEdit(screenId, op);
  // The mapping measures the picture in the composition, as the output does.
  const composition = useComposition();
  const [selected, setSelected] = useState<string | null>(null);
  // What the screen's handles and the panel below are for: a slice or a mask.
  const [focus, setFocus] = useState<"slice" | "mask">("slice");
  const [selectedMask, setSelectedMask] = useState<string | null>(null);
  const [point, setPoint] = useState(0);
  const [maskDraft, setMaskDraft] = useState<Mask | null>(null);
  const [pattern, setPattern] = useState(false);
  // The slice being dragged, as it looks before the backend has it.
  const [draft, setDraft] = useState<Slice | null>(null);
  const input = useRef<HTMLDivElement>(null);
  const output = useRef<HTMLDivElement>(null);

  useEffect(() => () => void showTestPattern(false), []);
  const current =
    slices.find((s) => s.id === selected) ?? slices[slices.length - 1] ?? null;
  const shown = (slice: Slice) => (draft?.id === slice.id ? draft : slice);
  const currentMask =
    masks.find((m) => m.id === selectedMask) ?? masks[masks.length - 1] ?? null;
  const shownMask = (mask: Mask) =>
    maskDraft?.id === mask.id ? maskDraft : mask;
  const editingMask = focus === "mask" && currentMask !== null;
  const picture: Size = { width: composition.width, height: composition.height };
  const display: Size = screen ?? DEFAULT_SIZE;
  const pictureAspect = `${picture.width} / ${picture.height}`;
  const screenAspect = `${display.width} / ${display.height}`;

  const moveRect = (
    slice: Slice,
    event: ReactPointerEvent<HTMLElement>,
    resize: boolean,
  ) => {
    setSelected(slice.id);
    setFocus("slice");
    let next = slice;
    drag(
      event,
      input.current,
      (d) => {
        const r = slice.input;
        const input: Rect = resize
          ? {
              ...r,
              width: Math.max(0.005, Math.min(1 - r.x, r.width + d.x)),
              height: Math.max(0.005, Math.min(1 - r.y, r.height + d.y)),
            }
          : {
              ...r,
              x: Math.max(0, Math.min(1 - r.width, r.x + d.x)),
              y: Math.max(0, Math.min(1 - r.height, r.y + d.y)),
            };
        next = { ...slice, input };
        setDraft(next);
      },
      () => {
        if (next !== slice)
          void edit({
            op: "setInput",
            id: slice.id,
            input: next.input,
          }).finally(() => setDraft(null));
        else setDraft(null);
      },
    );
  };

  const moveCorner = (
    slice: Slice,
    index: number,
    event: ReactPointerEvent<HTMLElement>,
  ) => {
    let next = slice;
    drag(
      event,
      output.current,
      (d) => {
        const corners = slice.corners.map((p, i) =>
          i === index ? { x: p.x + d.x, y: p.y + d.y } : p,
        ) as Slice["corners"];
        next = { ...slice, corners };
        setDraft(next);
      },
      () => {
        if (next !== slice)
          void edit({
            op: "setCorners",
            id: slice.id,
            corners: next.corners,
          }).finally(() => setDraft(null));
        else setDraft(null);
      },
    );
  };

  const moveQuad = (slice: Slice, event: ReactPointerEvent<Element>) => {
    setSelected(slice.id);
    setFocus("slice");
    let next = slice;
    drag(
      event,
      output.current,
      (d) => {
        next = {
          ...slice,
          corners: slice.corners.map((p) => ({
            x: p.x + d.x,
            y: p.y + d.y,
          })) as Slice["corners"],
        };
        setDraft(next);
      },
      () => {
        if (next !== slice)
          void edit({
            op: "setCorners",
            id: slice.id,
            corners: next.corners,
          }).finally(() => setDraft(null));
        else setDraft(null);
      },
    );
  };

  /** Drags one point of a mask, or the whole mask when `index` is null. */
  const moveMask = (
    mask: Mask,
    index: number | null,
    event: ReactPointerEvent<Element>,
  ) => {
    setSelectedMask(mask.id);
    setFocus("mask");
    if (index !== null) setPoint(index);
    let next = mask;
    drag(
      event,
      output.current,
      (d) => {
        next = {
          ...mask,
          points: mask.points.map((p, i) =>
            index === null || i === index ? { x: p.x + d.x, y: p.y + d.y } : p,
          ),
        };
        setMaskDraft(next);
      },
      () => {
        if (next !== mask)
          void editMask({
            op: "setPoints",
            id: mask.id,
            points: next.points,
          }).finally(() => setMaskDraft(null));
        else setMaskDraft(null);
      },
    );
  };

  return (
    <section className="mapping" aria-labelledby="mapping-title">
      <div className="mapping-bar">
        <h4 id="mapping-title">{t("setup.video.mapping")}</h4>
        <button
          type="button"
          className="button"
          onClick={() =>
            void edit({ op: "add" }).then(() => {
              setSelected(null);
              setFocus("slice");
            })
          }
        >
          {t("setup.video.addSlice")}
        </button>
        <button
          type="button"
          className="button"
          aria-pressed={pattern}
          onClick={() => {
            setPattern(!pattern);
            void showTestPattern(!pattern);
          }}
        >
          {t("setup.video.pattern")}
        </button>
        <button
          type="button"
          className="button"
          onClick={() =>
            void editMask({ op: "add" }).then(() => {
              setSelectedMask(null);
              setFocus("mask");
            })
          }
        >
          {t("setup.video.addMask")}
        </button>
        {slices.length > 0 && (
          <button
            type="button"
            className="button"
            onClick={() => void edit({ op: "clear" })}
          >
            {t("setup.video.clear")}
          </button>
        )}
      </div>
      <p className="field-note">
        {slices.length === 0
          ? t("setup.video.noSlices")
          : t("setup.video.mappingIntro")}
      </p>
      {(slices.length > 0 || masks.length > 0) && (
        <div className="mapping-stages">
          <figure className="mapping-stage">
            <figcaption>
              {t("setup.video.input")}{" "}
              <span className="mapping-size">
                {t("setup.video.size", { ...picture })}
              </span>
            </figcaption>
            <div
              className="mapping-canvas"
              ref={input}
              style={{ aspectRatio: pictureAspect }}
            >
              {slices.map((slice) => {
                const r = shown(slice).input;
                return (
                  <div
                    key={slice.id}
                    className="mapping-rect"
                    data-selected={slice.id === current?.id}
                    data-hidden={slice.hidden ?? false}
                    style={{
                      left: pct(r.x),
                      top: pct(r.y),
                      width: pct(r.width),
                      height: pct(r.height),
                    }}
                    onPointerDown={(e) => moveRect(slice, e, false)}
                  >
                    <span className="mapping-label">{slice.name}</span>
                    {slice.id === current?.id && (
                      <span
                        className="mapping-handle mapping-resize"
                        role="presentation"
                        title={t("setup.video.resize", { name: slice.name })}
                        onPointerDown={(e) => moveRect(slice, e, true)}
                      />
                    )}
                  </div>
                );
              })}
            </div>
          </figure>
          <figure className="mapping-stage">
            <figcaption>
              {t("setup.video.output")}{" "}
              <span className="mapping-size">
                {t("setup.video.size", { ...display })}
              </span>
            </figcaption>
            <div
              className="mapping-canvas"
              ref={output}
              style={{ aspectRatio: screenAspect }}
            >
              <svg
                className="mapping-quads"
                viewBox="0 0 1 1"
                preserveAspectRatio="none"
                aria-hidden="true"
              >
                {slices.map((slice) => (
                  <polygon
                    key={slice.id}
                    points={shown(slice)
                      .corners.map((p) => `${p.x},${p.y}`)
                      .join(" ")}
                    data-selected={slice.id === current?.id}
                    data-hidden={slice.hidden ?? false}
                    vectorEffect="non-scaling-stroke"
                    onPointerDown={(e) => moveQuad(slice, e)}
                  />
                ))}
                {masks.map((mask) => (
                  <polygon
                    key={mask.id}
                    className="mapping-mask"
                    points={shownMask(mask)
                      .points.map((p) => `${p.x},${p.y}`)
                      .join(" ")}
                    data-selected={editingMask && mask.id === currentMask?.id}
                    data-hidden={mask.hidden ?? false}
                    data-invert={mask.invert ?? false}
                    vectorEffect="non-scaling-stroke"
                    onPointerDown={(e) => moveMask(mask, null, e)}
                  />
                ))}
              </svg>
              {editingMask &&
                shownMask(currentMask).points.map((p, i) => (
                  <span
                    key={i}
                    className="mapping-handle mapping-point"
                    data-selected={i === point}
                    role="presentation"
                    title={t("setup.video.maskPoint", {
                      name: currentMask.name,
                      n: i + 1,
                    })}
                    style={{ left: pct(p.x), top: pct(p.y) }}
                    onPointerDown={(e) => moveMask(currentMask, i, e)}
                  />
                ))}
              {!editingMask &&
                current &&
                shown(current).corners.map((p, i) => (
                  <span
                    key={i}
                    className="mapping-handle mapping-corner"
                    data-corner={i}
                    role="presentation"
                    title={t("setup.video.corner", {
                      name: current.name,
                      n: i + 1,
                    })}
                    style={{ left: pct(p.x), top: pct(p.y) }}
                    onPointerDown={(e) => moveCorner(current, i, e)}
                  />
                ))}
            </div>
          </figure>
        </div>
      )}
      {editingMask && (
        <MaskFields
          screenId={screenId}
          mask={currentMask}
          point={point}
          setPoint={setPoint}
          onRemoved={() => setFocus("slice")}
        />
      )}
      {!editingMask && current && (
        <SliceFields screenId={screenId} slice={shown(current)} picture={picture} display={display} />
      )}
    </section>
  );
}
