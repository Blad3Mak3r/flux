import { useMemo, useRef } from "react";
import type { PointerEvent } from "react";
import { useT } from "../i18n";
import type { FrameRate } from "../project/types";
import { formatFrames } from "../timecode";
import { pickTicks } from "../timeline/scale";
import { RULER_H, patchReadout, readoutStore } from "../timeline/layout";
import { useStore } from "../store";

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
  const readout = useStore(readoutStore);

  const { path, labels } = useMemo(() => {
    const ticks = pickTicks(pxPerFrame, rate);
    const frames = Math.ceil(width / pxPerFrame);
    let d = "";
    const items: { x: number; text: string }[] = [];
    if (ticks.minor !== null) {
      for (let f = 0; f <= frames; f += ticks.minor) {
        if (f % ticks.major === 0) continue;
        const x = Math.round(f * pxPerFrame) + 0.5;
        d += `M${x} ${RULER_H - 7}V${RULER_H}`;
      }
    }
    for (let f = 0; f <= frames; f += ticks.major) {
      const x = Math.round(f * pxPerFrame) + 0.5;
      d += `M${x} ${RULER_H - 16}V${RULER_H}`;
      const full = formatFrames(start + f, rate);
      items.push({ x, text: ticks.wholeSeconds ? full.slice(0, 8) : full });
    }
    return { path: d, labels: items };
  }, [width, pxPerFrame, rate, start]);

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

  const { spanA, spanB } = readout;
  const spanStart = spanA !== null && spanB !== null ? Math.min(spanA, spanB) : null;
  const spanEnd = spanA !== null && spanB !== null ? Math.max(spanA, spanB) : null;

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
      {spanStart !== null && spanEnd !== null && spanEnd > spanStart && (
        <rect
          className="ruler-span"
          x={spanStart * pxPerFrame}
          y={0}
          width={(spanEnd - spanStart) * pxPerFrame}
          height={RULER_H}
        />
      )}
      {spanA !== null && <path className="ruler-cursor" d={`M${spanA * pxPerFrame + 0.5} 0V${RULER_H}`} />}
      {spanB !== null && spanB !== spanA && (
        <path className="ruler-cursor" d={`M${spanB * pxPerFrame + 0.5} 0V${RULER_H}`} />
      )}
      <path className="ruler-ticks" d={path} />
      {labels.map(({ x, text }) => (
        <text key={x} className="ruler-label" x={x + 5} y={RULER_H - 19}>
          {text}
        </text>
      ))}
    </svg>
  );
}
