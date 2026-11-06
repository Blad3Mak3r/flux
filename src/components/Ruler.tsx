import { memo, useRef } from "react";
import type { PointerEvent } from "react";
import { useT } from "../i18n";
import { snapFrame, snapTargets } from "../state/snap";
import type { FrameRate } from "../project/types";
import { formatFrames } from "../timecode";
import { barTickPositions, frameX, pickTicks, tickPositions } from "../timeline/scale";
import type { BeatGrid } from "../timeline/scale";
import type { SnapTarget } from "../timeline/snap";
import { RULER_H, patchReadout, useReadout } from "../timeline/layout";

/** The bar lines and beats, labelled with bar numbers (`3`) and, zoomed in, beats (`3.2`). */
const BarTicks = memo(function BarTicks({
  width,
  pxPerFrame,
  grid,
}: {
  width: number;
  pxPerFrame: number;
  grid: BeatGrid;
}) {
  const { major, minor } = barTickPositions(pxPerFrame, grid, width);
  let d = "";
  for (const f of minor) d += `M${frameX(f, pxPerFrame) + 0.5} ${RULER_H - 7}V${RULER_H}`;
  for (const { frame } of major) d += `M${frameX(frame, pxPerFrame) + 0.5} ${RULER_H - 16}V${RULER_H}`;
  return (
    <>
      <path className="ruler-ticks" d={d} />
      {major.map(({ frame, label }) => (
        <text key={frame} className="ruler-label" x={frameX(frame, pxPerFrame) + 5.5} y={RULER_H - 19}>
          {label}
        </text>
      ))}
    </>
  );
});

/** The ticks and their labels: a lot of nodes, redrawn only when the zoom, the width or the rate change. */
const RulerTicks = memo(function RulerTicks({
  width,
  pxPerFrame,
  rate,
  start,
}: {
  width: number;
  pxPerFrame: number;
  rate: FrameRate;
  start: number;
}) {
  const ticks = pickTicks(pxPerFrame, rate);
  const { major, minor } = tickPositions(pxPerFrame, rate, width);
  let d = "";
  for (const f of minor) d += `M${frameX(f, pxPerFrame) + 0.5} ${RULER_H - 7}V${RULER_H}`;
  const labels = major.map((f) => {
    const x = frameX(f, pxPerFrame) + 0.5;
    d += `M${x} ${RULER_H - 16}V${RULER_H}`;
    const full = formatFrames(start + f, rate);
    return { x, text: ticks.wholeSeconds ? full.slice(0, 8) : full };
  });
  return (
    <>
      <path className="ruler-ticks" d={d} />
      {labels.map(({ x, text }) => (
        <text key={x} className="ruler-label" x={x + 5} y={RULER_H - 19}>
          {text}
        </text>
      ))}
    </>
  );
});

/** The measured span, which moves with the pointer while the ticks stay where they are. */
function RulerSpan({ pxPerFrame }: { pxPerFrame: number }) {
  const spanA = useReadout((readout) => readout.spanA);
  const spanB = useReadout((readout) => readout.spanB);
  const spanStart = spanA !== null && spanB !== null ? Math.min(spanA, spanB) : null;
  const spanEnd = spanA !== null && spanB !== null ? Math.max(spanA, spanB) : null;
  return (
    <>
      {spanStart !== null && spanEnd !== null && spanEnd > spanStart && (
        <rect
          className="ruler-span"
          x={frameX(spanStart, pxPerFrame)}
          y={0}
          width={frameX(spanEnd, pxPerFrame) - frameX(spanStart, pxPerFrame)}
          height={RULER_H}
        />
      )}
      {spanA !== null && <path className="ruler-cursor" d={`M${frameX(spanA, pxPerFrame) + 0.5} 0V${RULER_H}`} />}
      {spanB !== null && spanB !== spanA && (
        <path className="ruler-cursor" d={`M${frameX(spanB, pxPerFrame) + 0.5} 0V${RULER_H}`} />
      )}
    </>
  );
}

/**
 * The time axis. Ticks are drawn as one path; labels carry the project timecode, so the start
 * offset is already in every number. With a `grid` the axis counts bars and beats instead. Click or
 * drag to move the playhead; hold Shift and drag to measure the span between two points.
 */
export function Ruler({
  width,
  pxPerFrame,
  rate,
  start,
  grid,
  onSeek,
}: {
  width: number;
  pxPerFrame: number;
  rate: FrameRate;
  start: number;
  /** The beat grid to count in, or `null` for timecode. */
  grid: BeatGrid | null;
  onSeek: (relativeFrame: number) => void;
}) {
  const t = useT();
  const mode = useRef<"seek" | "measure" | null>(null);
  // What the playhead or the span can land on, taken when the press starts; not the playhead itself.
  const targets = useRef<SnapTarget[]>([]);

  const frameAt = (event: PointerEvent<SVGSVGElement>) => {
    const left = event.currentTarget.getBoundingClientRect().left;
    return Math.max(0, snapFrame(Math.round((event.clientX - left) / pxPerFrame), targets.current));
  };

  const onPointerDown = (event: PointerEvent<SVGSVGElement>) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    targets.current = snapTargets([], ["playhead"]);
    const frame = frameAt(event);
    if (event.shiftKey) {
      mode.current = "measure";
      patchReadout({ spanA: frame, spanB: frame });
    } else {
      mode.current = "seek";
      patchReadout({ spanA: null, spanB: null });
      onSeek(frame);
    }
  };

  const onPointerMove = (event: PointerEvent<SVGSVGElement>) => {
    const frame = frameAt(event);
    patchReadout({ hover: frame });
    if (mode.current === "seek") onSeek(frame);
    else if (mode.current === "measure") patchReadout({ spanB: frame });
  };

  return (
    <svg
      className="ruler"
      width={width}
      height={RULER_H}
      role="group"
      aria-label={t("timeline.ruler")}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={() => (mode.current = null)}
      onPointerCancel={() => (mode.current = null)}
    >
      <RulerSpan pxPerFrame={pxPerFrame} />
      {grid ? (
        <BarTicks width={width} pxPerFrame={pxPerFrame} grid={grid} />
      ) : (
        <RulerTicks width={width} pxPerFrame={pxPerFrame} rate={rate} start={start} />
      )}
    </svg>
  );
}
