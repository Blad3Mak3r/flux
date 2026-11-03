import { memo, useRef } from "react";
import type { PointerEvent } from "react";
import { useT } from "../i18n";
import type { FrameRate } from "../project/types";
import { formatFrames } from "../timecode";
import { frameX, pickTicks, tickPositions } from "../timeline/scale";
import { RULER_H, patchReadout, useReadout } from "../timeline/layout";

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
 * offset is already in every number. Click or drag to move the playhead; hold Shift and drag to
 * measure the span between two points.
 */
export function Ruler({
  width,
  pxPerFrame,
  rate,
  start,
  onSeek,
}: {
  width: number;
  pxPerFrame: number;
  rate: FrameRate;
  start: number;
  onSeek: (relativeFrame: number) => void;
}) {
  const t = useT();
  const mode = useRef<"seek" | "measure" | null>(null);

  const frameAt = (event: PointerEvent<SVGSVGElement>) => {
    const left = event.currentTarget.getBoundingClientRect().left;
    return Math.max(0, Math.round((event.clientX - left) / pxPerFrame));
  };

  const onPointerDown = (event: PointerEvent<SVGSVGElement>) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
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
      <RulerTicks width={width} pxPerFrame={pxPerFrame} rate={rate} start={start} />
    </svg>
  );
}
