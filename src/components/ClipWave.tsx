import { useEffect, useRef } from "react";
import type { Peaks } from "../state/doc";
import { drawInWorker, workerDrawing } from "../timeline/wavePool";
import { canvasSize, renderWave } from "../timeline/waveform";
import type { WaveJob } from "../timeline/waveform";

/**
 * A clip's waveform. The drawing happens in a worker when the webview can do that, so zooming a
 * timeline of many clips never blocks the page; otherwise it is drawn here, the same way.
 */
export function ClipWave({
  seed,
  width,
  height,
  color,
  peaks,
  pxPerSecond,
  startSeconds,
  tint,
}: {
  seed: string;
  width: number;
  height: number;
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

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const { dpr, pixelWidth, pixelHeight } = canvasSize(width, height, window.devicePixelRatio);
    // A canvas does not follow `currentColor` reliably; resolve the colour it inherits.
    const job: WaveJob = {
      seed,
      width,
      height,
      dpr,
      color: color === "currentColor" ? getComputedStyle(canvas).color : color,
      pxPerSecond,
      startSeconds,
    };

    if (workerDrawing) return drawInWorker(canvas, job, peaks);

    canvas.width = pixelWidth;
    canvas.height = pixelHeight;
    const context = canvas.getContext("2d");
    if (context) renderWave(context, job, peaks);
  }, [seed, width, height, color, peaks, pxPerSecond, startSeconds, tint]);

  return <canvas ref={ref} className="clip-wave" style={{ width, height }} aria-hidden="true" />;
}
