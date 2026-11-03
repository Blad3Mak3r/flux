// A few workers that draw clip waveforms, so zooming a timeline of many clips neither blocks the page
// nor draws them one after another. Each canvas keeps to one worker, which keeps its peaks.

import type { Peaks } from "../state/doc";
import type { WaveJob, WaveReply, WaveRequest } from "./waveform";

const MAX_WORKERS = 4;

interface Lane {
  worker: Worker;
  /** Peaks this worker already holds, by key. */
  has: Set<number>;
}

let lanes: Lane[] | null = null;
let next = 0;
let drawing = 0;

/** The canvas each drawing is for; a reply for one that was superseded is dropped. */
const latest = new WeakMap<HTMLCanvasElement, number>();
const targets = new Map<number, HTMLCanvasElement>();
const laneOf = new WeakMap<HTMLCanvasElement, Lane>();

const keyOf = new WeakMap<Peaks, number>();
let keys = 0;
/** Workers keep the peaks they were sent; tell them to let go once the page no longer does. */
const forgotten = new FinalizationRegistry<number>((key) => {
  lanes?.forEach(({ worker, has }) => {
    if (has.delete(key)) worker.postMessage({ type: "forget", key } satisfies WaveRequest);
  });
});

/** Whether this webview can draw a canvas in a worker and hand the result back as a bitmap. */
export const workerDrawing: boolean = (() => {
  try {
    if (typeof Worker === "undefined" || typeof OffscreenCanvas === "undefined") return false;
    const probe = new OffscreenCanvas(1, 1);
    return (
      probe.getContext("2d") !== null &&
      typeof probe.transferToImageBitmap === "function" &&
      document.createElement("canvas").getContext("bitmaprenderer") !== null
    );
  } catch {
    return false;
  }
})();

function start(): Lane[] {
  const count = Math.max(1, Math.min(MAX_WORKERS, (navigator.hardwareConcurrency || 2) - 1));
  return Array.from({ length: count }, () => {
    const worker = new Worker(new URL("./wave.worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = ({ data }: MessageEvent<WaveReply>) => {
      const canvas = targets.get(data.id);
      targets.delete(data.id);
      if (!canvas || latest.get(canvas) !== data.id) return data.bitmap.close();
      canvas.getContext("bitmaprenderer")?.transferFromImageBitmap(data.bitmap);
    };
    worker.onerror = (event) => console.error("waveform worker:", event.message);
    return { worker, has: new Set<number>() };
  });
}

function keyFor(peaks: Peaks, lane: Lane): number {
  let key = keyOf.get(peaks);
  if (key === undefined) {
    key = ++keys;
    keyOf.set(peaks, key);
    forgotten.register(peaks, key);
  }
  if (!lane.has.has(key)) {
    const values = Int8Array.from(peaks.values);
    lane.has.add(key);
    lane.worker.postMessage(
      { type: "peaks", key, values, perSecond: peaks.perSecond } satisfies WaveRequest,
      [values.buffer],
    );
  }
  return key;
}

/**
 * Draws `job` on `canvas` in a worker and shows it when it is done. Returns what to call when a newer
 * drawing, or the end of the canvas, makes this one pointless.
 */
export function drawInWorker(canvas: HTMLCanvasElement, job: WaveJob, peaks: Peaks | undefined): () => void {
  lanes ??= start();
  let lane = laneOf.get(canvas);
  if (!lane) {
    lane = lanes[next++ % lanes.length];
    laneOf.set(canvas, lane);
  }
  const id = ++drawing;
  latest.set(canvas, id);
  targets.set(id, canvas);
  lane.worker.postMessage({ type: "draw", id, job, peaks: peaks ? keyFor(peaks, lane) : null } satisfies WaveRequest);
  return () => {
    // Superseded or unmounted: its reply, if it still comes, is closed instead of drawn.
    if (latest.get(canvas) === id) latest.delete(canvas);
  };
}
