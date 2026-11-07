// Draws a clip's waveform. Pure: it only needs a 2D context, so the same code runs in a worker, on an
// `OffscreenCanvas`, and, where that is not available, on the page's own canvas.

export type Context2D = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

/** `[min, max]` signed-byte pairs, `perSecond` of them per second. */
export interface PeakData {
  values: ArrayLike<number>;
  perSecond: number;
}

/** What one drawing depends on. Plain data, so it can cross to a worker. */
export interface WaveJob {
  /** Seeds the placeholder shown until the real waveform is known. */
  seed: string;
  /** CSS pixels of the canvas, which holds the part of the wave that starts at `x0`. */
  width: number;
  height: number;
  /** How many pixels from the start of the wave the canvas begins. */
  x0: number;
  /** Device pixels per CSS pixel the canvas is drawn at. */
  dpr: number;
  color: string;
  pxPerSecond: number;
  /** Seconds of the audio the drawing starts at, for a clip that is trimmed in. */
  startSeconds: number;
}

/** Requests the page sends a waveform worker, and what comes back. */
export type WaveRequest =
  | { type: "peaks"; key: number; values: Int8Array; perSecond: number }
  | { type: "forget"; key: number }
  | { type: "draw"; id: number; job: WaveJob; peaks: number | null };

export interface WaveReply {
  id: number;
  bitmap: ImageBitmap;
}

/** What of the timeline is on screen, in pixels from its origin. */
export interface Viewport {
  left: number;
  width: number;
}

/** Pixels of wave drawn at a time: scrolling redraws a clip once per chunk, not at every pixel. */
const SLICE_CHUNK = 512;

/**
 * The part of a wave that is worth drawing: the `width` pixels of a clip whose audio starts `origin`
 * pixels from the origin of the timeline, cut to what is on screen plus a screenful on each side,
 * in whole chunks. A canvas that only holds that stays small however long the clip is or far in it
 * the view is, so it is never drawn below the screen's resolution. `x1 === x0` means nothing shows.
 */
export function waveSlice(origin: number, width: number, view: Viewport): { x0: number; x1: number } {
  const margin = Math.max(view.width, SLICE_CHUNK);
  const clamp = (x: number) => Math.min(Math.max(x, 0), width);
  const x0 = clamp(Math.floor((view.left - margin - origin) / SLICE_CHUNK) * SLICE_CHUNK);
  const x1 = clamp(Math.ceil((view.left + view.width + margin - origin) / SLICE_CHUNK) * SLICE_CHUNK);
  return x1 > x0 ? { x0, x1 } : { x0: 0, x1: 0 };
}

/** Longest canvas the browsers draw reliably; wider ones are drawn at a lower resolution. */
const MAX_CANVAS_PX = 16_000;

/** The pixel size of the canvas for a size in CSS pixels, lowering the resolution when it is too wide. */
export function canvasSize(width: number, height: number, devicePixelRatio: number) {
  const dpr = Math.min(devicePixelRatio || 1, MAX_CANVAS_PX / Math.max(width, 1));
  return { dpr, pixelWidth: Math.max(1, Math.floor(width * dpr)), pixelHeight: Math.max(1, Math.floor(height * dpr)) };
}

// A placeholder waveform: a deterministic envelope seeded by the asset id, so the same clip always
// looks the same.
function seeded(seed: string): () => number {
  let h = 2166136261;
  for (const char of seed) h = Math.imul(h ^ char.charCodeAt(0), 16777619);
  return () => {
    h = Math.imul(h ^ (h >>> 15), 2246822507);
    h = Math.imul(h ^ (h >>> 13), 3266489909);
    return ((h ^= h >>> 16) >>> 0) / 4294967296;
  };
}

/** One column of pixels per peak window it covers: the lowest low and the highest high. */
function drawPeaks(
  context: Context2D,
  { values, perSecond }: PeakData,
  { width, height, x0, pxPerSecond, startSeconds }: WaveJob,
) {
  const pairs = values.length / 2;
  const mid = height / 2;
  const reach = mid - 3;
  for (let x = 0; x < width; x += 1) {
    // `x` is on the canvas; the audio it shows is `x0` pixels further along.
    const first = Math.floor((startSeconds + (x0 + x) / pxPerSecond) * perSecond);
    const last = Math.max(first + 1, Math.ceil((startSeconds + (x0 + x + 1) / pxPerSecond) * perSecond));
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

function drawPlaceholder(context: Context2D, { seed, width, height, x0 }: WaveJob) {
  const random = seeded(seed);
  const mid = height / 2;
  let envelope = 0.4;
  // The sequence runs from the start of the wave, so a slice of it is the same as in the whole.
  for (let x = 0; x < x0 + width; x += 2) {
    envelope = Math.min(0.95, Math.max(0.12, envelope + (random() - 0.5) * 0.12));
    const amplitude = envelope * (0.45 + random() * 0.55) * (mid - 4);
    if (x >= x0) context.fillRect(x - x0, mid - amplitude, 1, Math.max(1, amplitude * 2));
  }
}

/** Clears the canvas, which the caller has sized with `canvasSize`, and draws the waveform on it. */
export function renderWave(context: Context2D, job: WaveJob, peaks?: PeakData): void {
  context.setTransform(job.dpr, 0, 0, job.dpr, 0, 0);
  context.clearRect(0, 0, job.width, job.height);
  context.fillStyle = job.color;
  if (peaks) drawPeaks(context, peaks, job);
  else drawPlaceholder(context, job);
}
