import { useEffect, useRef, useState } from "react";
import type { Peaks } from "../state/doc";
import { useViewport } from "../timeline/layout";
import { drawInWorker, workerDrawing } from "../timeline/wavePool";
import { canvasSize, renderWave, waveSlice } from "../timeline/waveform";
import type { WaveJob } from "../timeline/waveform";

/** The screen's pixel ratio, which changes when the window moves to another monitor or the page is zoomed. */
function useDevicePixelRatio(): number {
  const [ratio, setRatio] = useState(window.devicePixelRatio || 1);
  useEffect(() => {
    const query = window.matchMedia(`(resolution: ${ratio}dppx)`);
    const onChange = () => setRatio(window.devicePixelRatio || 1);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, [ratio]);
  return ratio;
}

/**
 * A clip's waveform. Only the part that is on screen, with a screenful each side, is drawn, so a
 * clip of any length is as sharp as the screen allows and costs no more than one that fits. The
 * drawing happens in a worker when the webview can do that, so zooming a timeline of many clips
 * never blocks the page; otherwise it is drawn here, the same way.
 */
export function ClipWave({
  seed,
  width,
  height,
  origin,
  color,
  peaks,
  pxPerSecond,
  startSeconds,
  tint,
}: {
  seed: string;
  /** Pixels the whole wave spans, of which only a slice is drawn. */
  width: number;
  height: number;
  /** Pixels from the origin of the timeline to the start of the wave. */
  origin: number;
  color: string;
  /** Real waveform; without it a placeholder is drawn. */
  peaks?: Peaks;
  pxPerSecond: number;
  /** Seconds of the audio the drawing starts at: where the clip is trimmed in. */
  startSeconds: number;
  /** Changes whenever the colour `currentColor` resolves to does, so the canvas is drawn again. */
  tint?: string;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const dpr = useDevicePixelRatio();
  // Each is a number that only changes when the view crosses into another chunk, so scrolling
  // re-renders the clip once per chunk and not on every pixel.
  const x0 = useViewport((view) => waveSlice(origin, width, view).x0);
  const x1 = useViewport((view) => waveSlice(origin, width, view).x1);
  const sliceWidth = x1 - x0;

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas || sliceWidth <= 0) return;
    const size = canvasSize(sliceWidth, height, dpr);
    // A canvas does not follow `currentColor` reliably; resolve the colour it inherits.
    const job: WaveJob = {
      seed,
      width: sliceWidth,
      height,
      x0,
      dpr: size.dpr,
      color: color === "currentColor" ? getComputedStyle(canvas).color : color,
      pxPerSecond,
      startSeconds,
    };

    if (workerDrawing) return drawInWorker(canvas, job, peaks);

    canvas.width = size.pixelWidth;
    canvas.height = size.pixelHeight;
    const context = canvas.getContext("2d");
    if (context) renderWave(context, job, peaks);
  }, [seed, sliceWidth, x0, height, dpr, color, peaks, pxPerSecond, startSeconds, tint]);

  return (
    <canvas
      ref={ref}
      className="clip-wave"
      hidden={sliceWidth <= 0}
      style={{ left: x0, width: sliceWidth, height }}
      aria-hidden="true"
    />
  );
}
