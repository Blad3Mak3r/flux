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

export function ClipWave({
  seed,
  width,
  height,
  color,
}: {
  seed: string;
  width: number;
  height: number;
  color: string;
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
    context.fillStyle = color;

    const random = seeded(seed);
    const mid = height / 2;
    let envelope = 0.4;
    for (let x = 0; x < width; x += 2) {
      // A slow drift keeps phrases; the noise on top is the "signal".
      envelope = Math.min(0.95, Math.max(0.12, envelope + (random() - 0.5) * 0.12));
      const amplitude = envelope * (0.45 + random() * 0.55) * (mid - 4);
      context.fillRect(x, mid - amplitude, 1, Math.max(1, amplitude * 2));
    }
  }, [seed, width, height, color]);

  return <canvas ref={ref} className="clip-wave" style={{ width, height }} aria-hidden="true" />;
}
