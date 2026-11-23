import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { useT } from "../i18n";
import { TextField } from "../components/fields";
import { useDoc } from "../state/store";
import { mappingEdit, showTestPattern } from "../video/mapping";
import type { MappingOp } from "../video/mapping";
import type { Point, Rect, Size, Slice } from "../video/mapping";

const NO_SLICES: Slice[] = [];

/** Where the pointer is over `box`, as 0 to 1 of its width and height. */
function at(
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
function drag(
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
/** A fraction of `of` pixels, as the whole number of pixels shown in a field. */
const px = (v: number, of: number) => String(Math.round(v * of));
const DEFAULT_SIZE: Size = { width: 1920, height: 1080 };
const CORNERS = ["topLeft", "topRight", "bottomRight", "bottomLeft"] as const;

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
  const edit = (op: MappingOp) => mappingEdit(screenId, op);
  // The mapping measures the picture in the size of the project's first video, as the output does.
  const video = useDoc((doc) => doc.video[doc.project.videos?.[0]?.id ?? ""]);
  const [selected, setSelected] = useState<string | null>(null);
  const [pattern, setPattern] = useState(false);
  // The slice being dragged, as it looks before the backend has it.
  const [draft, setDraft] = useState<Slice | null>(null);
  const input = useRef<HTMLDivElement>(null);
  const output = useRef<HTMLDivElement>(null);

  useEffect(() => () => void showTestPattern(false), []);
  const current =
    slices.find((s) => s.id === selected) ?? slices[slices.length - 1] ?? null;
  const shown = (slice: Slice) => (draft?.id === slice.id ? draft : slice);
  const picture: Size = video
    ? { width: video.width, height: video.height }
    : DEFAULT_SIZE;
  const display: Size = screen ?? DEFAULT_SIZE;
  const pictureAspect = `${picture.width} / ${picture.height}`;
  const screenAspect = `${display.width} / ${display.height}`;

  const moveRect = (
    slice: Slice,
    event: ReactPointerEvent<HTMLElement>,
    resize: boolean,
  ) => {
    setSelected(slice.id);
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

  return (
    <section className="mapping" aria-labelledby="mapping-title">
      <div className="mapping-bar">
        <h4 id="mapping-title">{t("setup.video.mapping")}</h4>
        <button
          type="button"
          className="button"
          onClick={() => void edit({ op: "add" })}
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
      {slices.length > 0 && (
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
              </svg>
              {current &&
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
      {current && (
        <div className="mapping-slice">
          <TextField
            label={t("setup.video.sliceName")}
            value={current.name}
            onCommit={(name) =>
              void edit({ op: "rename", id: current.id, name })
            }
          />
          <fieldset className="mapping-values">
            <legend>{t("setup.video.inputValues")}</legend>
            {(["x", "y", "width", "height"] as const).map((field) => (
              <TextField
                key={field}
                label={t(`setup.video.field.${field}`)}
                inputMode="numeric"
                monospace
                value={px(
                  shown(current).input[field],
                  field === "x" || field === "width"
                    ? picture.width
                    : picture.height,
                )}
                onCommit={(text) =>
                  void edit({
                    op: "setInputValue",
                    id: current.id,
                    field,
                    text,
                    size: picture,
                  })
                }
              />
            ))}
          </fieldset>
          <fieldset className="mapping-values mapping-corners">
            <legend>{t("setup.video.cornerValues")}</legend>
            {CORNERS.map((name, corner) =>
              (["x", "y"] as const).map((axis) => (
                <TextField
                  key={`${name}-${axis}`}
                  label={t(`setup.video.cornerAxis.${axis}`, {
                    corner: t(`setup.video.cornerName.${name}`),
                  })}
                  inputMode="numeric"
                  monospace
                  value={px(
                    shown(current).corners[corner][axis],
                    axis === "x" ? display.width : display.height,
                  )}
                  onCommit={(text) =>
                    void edit({
                      op: "setCornerValue",
                      id: current.id,
                      corner,
                      axis,
                      text,
                      size: display,
                    })
                  }
                />
              )),
            )}
          </fieldset>
          <div className="button-row">
            <button
              type="button"
              className="button"
              aria-pressed={current.hidden ?? false}
              onClick={() =>
                void edit({
                  op: "setHidden",
                  id: current.id,
                  hidden: !current.hidden,
                })
              }
            >
              {t("setup.video.hide")}
            </button>
            <button
              type="button"
              className="button"
              onClick={() => void edit({ op: "resetCorners", id: current.id })}
            >
              {t("setup.video.resetCorners")}
            </button>
            <button
              type="button"
              className="button danger"
              onClick={() => void edit({ op: "remove", id: current.id })}
            >
              {t("setup.video.removeSlice")}
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
