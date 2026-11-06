import { useRef, useState } from "react";
import type { KeyboardEvent, MouseEvent, PointerEvent } from "react";
import { useT } from "../i18n";
import { dbToFader, faderToDb, formatDb } from "../audio/level";
import {
  ENVELOPE_CURVES,
  clampEnvelopeDb,
  envelopeLine,
  insertPoint,
  movePoint,
  removePoint,
  setPointCurve,
} from "../audio/envelope";
import { ENVELOPE_TICKS_PER_FRAME } from "../project/types";
import type { AudioClip, EnvPoint } from "../project/types";
import { setClipEnvelope } from "../state/actions";
import { snapFrame, snapTargets } from "../state/snap";
import { menuAtPointer, setSnapGuide } from "../state/ui";
import type { MenuItem } from "../state/ui";
import type { SnapTarget } from "../timeline/snap";

/** Side of the square that marks a point, in pixels. */
const POINT = 8;

interface Drag {
  index: number;
  x: number;
  y: number;
  at: number;
  db: number;
  targets: SnapTarget[];
}

/**
 * The volume drawn over a clip. The line shows when the clip has an envelope; on the selected clip
 * its points can be dragged (Shift for fine steps, Alt to turn the magnet the other way, Esc to
 * cancel), added or removed with a double click, and moved with the arrow keys. Points are counted
 * from the start of the audio file, so the clip moving or being trimmed leaves them where they are.
 * A drag is held here and handed over once, on release, so it is one step of undo.
 */
export function EnvelopeLayer({
  clip,
  shown,
  left,
  width,
  height,
  pxPerFrame,
  selected,
}: {
  clip: AudioClip;
  /** Where the clip starts on screen right now, in frames: it moves while it is dragged. */
  shown: number;
  /** The frame at the left edge of the clip's box. */
  left: number;
  width: number;
  height: number;
  pxPerFrame: number;
  selected: boolean;
}) {
  const t = useT();
  const stored = clip.envelope ?? [];
  const [draft, setDraft] = useState<EnvPoint[] | null>(null);
  const [active, setActive] = useState<number | null>(null);
  const drag = useRef<Drag | null>(null);
  const points = draft ?? stored;
  const offset = clip.offset ?? 0;

  const xOf = (at: number) => (shown - offset + at / ENVELOPE_TICKS_PER_FRAME - left) * pxPerFrame;
  const yOf = (db: number) => (1 - dbToFader(db)) * height;
  const dbAt = (y: number) => faderToDb(Math.min(1, Math.max(0, 1 - y / height)));

  if (points.length === 0 && !selected) return null;

  const line = envelopeLine(points)
    .map(({ at, db }) => `${Math.round(xOf(at) * 100) / 100},${Math.round(yOf(db) * 100) / 100}`)
    .join(" ");

  const finish = (commit: boolean) => {
    if (commit && draft) setClipEnvelope(clip.id, draft);
    drag.current = null;
    setDraft(null);
    setActive(null);
    setSnapGuide(null);
  };

  const onPointDown = (event: PointerEvent<SVGRectElement>, index: number) => {
    if (event.button !== 0) return;
    event.stopPropagation(); // The clip underneath would start moving.
    event.currentTarget.setPointerCapture(event.pointerId);
    event.currentTarget.focus();
    const point = stored[index];
    drag.current = {
      index,
      x: event.clientX,
      y: event.clientY,
      at: point.at,
      db: point.gainDb,
      targets: snapTargets([clip.id]),
    };
    setActive(index);
  };

  const onPointMove = (event: PointerEvent<SVGRectElement>) => {
    const start = drag.current;
    if (!start) return;
    const fine = event.shiftKey ? 0.2 : 1;
    const frames = ((event.clientX - start.x) / pxPerFrame) * fine;
    // Frames on the timeline, from the origin; the magnet works on whole ones, and only for a full-speed drag.
    let timeline = clip.start - offset + start.at / ENVELOPE_TICKS_PER_FRAME + frames;
    if (fine === 1) {
      const whole = Math.round(timeline);
      const snapped = snapFrame(whole, start.targets, { alt: event.altKey, guide: true });
      if (snapped !== whole) timeline = snapped;
    } else {
      setSnapGuide(null);
    }
    const at = (timeline - clip.start + offset) * ENVELOPE_TICKS_PER_FRAME;
    const position = dbToFader(start.db) - ((event.clientY - start.y) / height) * fine;
    setDraft(movePoint(stored, start.index, at, faderToDb(Math.min(1, Math.max(0, position)))));
  };

  const onPointUp = () => {
    const unchanged = !draft || draft === stored;
    finish(!unchanged);
  };

  const onPointKey = (event: KeyboardEvent<SVGRectElement>, index: number) => {
    const point = stored[index];
    if (event.key === "Escape" && drag.current) {
      event.stopPropagation();
      event.preventDefault();
      finish(false);
      return;
    }
    const big = event.shiftKey;
    const step: Record<string, [number, number]> = {
      ArrowLeft: [big ? -10 : -1, 0],
      ArrowRight: [big ? 10 : 1, 0],
      ArrowUp: [0, big ? 3 : 0.5],
      ArrowDown: [0, big ? -3 : -0.5],
    };
    if (event.key === "Delete" || event.key === "Backspace") {
      event.stopPropagation();
      event.preventDefault();
      setClipEnvelope(clip.id, removePoint(stored, index));
    } else if (step[event.key]) {
      event.stopPropagation();
      event.preventDefault();
      const [frames, db] = step[event.key];
      setClipEnvelope(clip.id, movePoint(stored, index, point.at + frames * ENVELOPE_TICKS_PER_FRAME, point.gainDb + db));
    }
  };

  const onAdd = (event: MouseEvent<SVGRectElement>) => {
    event.stopPropagation();
    const box = event.currentTarget.getBoundingClientRect();
    const timeline = left + (event.clientX - box.left) / pxPerFrame;
    const snapped = snapFrame(Math.round(timeline), snapTargets([clip.id]));
    const frame = snapped !== Math.round(timeline) ? snapped : timeline;
    const at = (frame - clip.start + offset) * ENVELOPE_TICKS_PER_FRAME;
    setClipEnvelope(clip.id, insertPoint(stored, at, dbAt(event.clientY - box.top)));
  };

  const onPointMenu = (event: MouseEvent<SVGRectElement>, index: number) => {
    const items: MenuItem[] = [
      ...ENVELOPE_CURVES.map((curve): MenuItem => ({
        label: t(`envelope.curve.${curve}`),
        checked: (stored[index].curve ?? "linear") === curve,
        run: () => setClipEnvelope(clip.id, setPointCurve(stored, index, curve)),
      })),
      "-",
      { label: t("envelope.removePoint"), run: () => setClipEnvelope(clip.id, removePoint(stored, index)) },
    ];
    menuAtPointer(event, items);
  };

  return (
    <svg className="clip-envelope" width={width} height={height} data-active={selected} aria-hidden={!selected}>
      {selected && <rect className="envelope-hit" width={width} height={height} onDoubleClick={onAdd} />}
      {points.length > 0 && <polyline className="envelope-line" points={line} />}
      {selected &&
        points.map((point, index) => {
          const x = xOf(point.at);
          if (x < -POINT || x > width + POINT) return null;
          const y = yOf(point.gainDb);
          return (
            // By place in the list, not by `at`: that changes while the point is dragged, and the
            // point must keep its pointer capture.
            <g key={index}>
              <rect
                className="envelope-point"
                x={x - POINT / 2}
                y={Math.min(Math.max(y, POINT / 2), height - POINT / 2) - POINT / 2}
                width={POINT}
                height={POINT}
                role="button"
                tabIndex={0}
                data-active={active === index}
                aria-label={t("envelope.point", {
                  index: index + 1,
                  count: points.length,
                  db: formatDb(point.gainDb),
                })}
                onPointerDown={(event) => onPointDown(event, index)}
                onPointerMove={onPointMove}
                onPointerUp={onPointUp}
                onPointerCancel={() => finish(false)}
                onKeyDown={(event) => onPointKey(event, index)}
                onDoubleClick={(event) => {
                  event.stopPropagation();
                  setClipEnvelope(clip.id, removePoint(stored, index));
                }}
                onContextMenu={(event) => onPointMenu(event, index)}
              />
              {active === index && (
                <text className="envelope-readout" x={Math.min(x + 8, width - 44)} y={Math.max(y - 6, 12)}>
                  {formatDb(clampEnvelopeDb(point.gainDb))} dB
                </text>
              )}
            </g>
          );
        })}
    </svg>
  );
}
