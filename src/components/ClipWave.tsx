import { useEffect, useRef } from "react";

// A placeholder waveform: a deterministic envelope seeded by the asset id, so the same clip always
// looks the same. Real peaks come from the backend once clip playback exists.
function seeded(seed: string): () => number {
  let h = 2166136261;
  for (const char of seed) h = Math.imul(h ^ char.charCodeAt(0), 16777619);
  return () => {
    h = Math.imul(h ^ (h >>> 15), 2246822507);
    h = Math.imul(h ^ (h >>> 13), 3266489909);
    return ((h ^= h >>> 16) >>> 0) / 4294967296;
  };
}

const MAX_CANVAS_PX = 16_000;

/** One column of pixels per peak window it covers: the lowest low and the highest high. */
function drawPeaks(
  context: CanvasRenderingContext2D,
  { values, perSecond }: Peaks,
  width: number,
  height: number,
  pxPerSecond: number,
) {
  const pairs = values.length / 2;
  const mid = height / 2;
  const reach = mid - 3;
  for (let x = 0; x < width; x += 1) {
    const first = Math.floor((x / pxPerSecond) * perSecond);
    const last = Math.max(first + 1, Math.ceil(((x + 1) / pxPerSecond) * perSecond));
    if (first >= pairs) break;
    let low = 127;
    let high = -128;
    for (let i = first; i < Math.min(last, pairs); i += 1) {
      low = Math.min(low, values[i * 2]);
      high = Math.max(high, values[i * 2 + 1]);
    }
    const top = mid - (Math.max(high, 0) / 127) * reach;
    const bottom = mid + (Math.max(-low, 0) / 128) * reach;
    context.fillRect(x, top, 1, Math.max(1, bottom - top));
  }
}

/** The waveform the backend decoded: `[min, max]` byte pairs, `perSecond` of them per second. */
export interface Peaks {
  values: number[];
  perSecond: number;
}

export function ClipWave({
  seed,
  width,
  height,
  color,
  peaks,
  pxPerSecond,
}: {
  seed: string;
  width: number;
  height: number;
  color: string;
  /** Real waveform; without it a placeholder is drawn. */
  peaks?: Peaks;
  pxPerSecond: number;
}) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    const dpr = Math.min(window.devicePixelRatio || 1, MAX_CANVAS_PX / Math.max(width, 1));
    canvas.width = Math.max(1, Math.floor(width * dpr));
    canvas.height = Math.max(1, Math.floor(height * dpr));
    context.scale(dpr, dpr);
    context.clearRect(0, 0, width, height);
    // A canvas does not follow `currentColor` reliably; resolve the colour it inherits.
    context.fillStyle = color === "currentColor" ? getComputedStyle(canvas).color : color;

    if (peaks) {
      drawPeaks(context, peaks, width, height, pxPerSecond);
      return;
    }
    const random = seeded(seed);
    const mid = height / 2;
    let envelope = 0.4;
    for (let x = 0; x < width; x += 2) {
      // A slow drift keeps phrases; the noise on top is the "signal".
      envelope = Math.min(0.95, Math.max(0.12, envelope + (random() - 0.5) * 0.12));
      const amplitude = envelope * (0.45 + random() * 0.55) * (mid - 4);
      context.fillRect(x, mid - amplitude, 1, Math.max(1, amplitude * 2));
    }
  }, [seed, width, height, color, peaks, pxPerSecond]);

  return <canvas ref={ref} className="clip-wave" style={{ width, height }} aria-hidden="true" />;
}
