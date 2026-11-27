// What the backend is doing to an imported file (probing, thumbnails, decoding, tempo), told by
// `media-progress` events: the clips of the asset draw it as a thin loading bar.

import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";

interface Payload {
  asset: string;
  stage: string;
  /** 0 to 1, or `null` while the step cannot tell how far it is. */
  fraction: number | null;
  done: boolean;
}

/** An asset can be in several steps at once (thumbnails and sound); the bar shows their pace together. */
interface Work {
  stages: Record<string, number | null>;
  /** The last stage announced, for the bar's label. */
  stage: string;
  /** How far the bar is, 0 to 1; `null` when no running step can tell how far it is. */
  shown: number | null;
}

const useProgress = create<Record<string, Work>>(() => ({}));
const timers = new Map<string, number>();
/** A step that ends and the next that starts a moment later must not make the bar blink. */
const LINGER_MS = 400;

function update({ asset, stage, fraction, done }: Payload): void {
  window.clearTimeout(timers.get(asset));
  const before = useProgress.getState()[asset];
  const { [stage]: _gone, ...others } = before?.stages ?? {};
  const stages = done ? others : { ...others, [stage]: fraction };
  if (Object.keys(stages).length === 0) {
    timers.set(
      asset,
      window.setTimeout(() => useProgress.setState(({ [asset]: _ended, ...rest }) => rest, true), LINGER_MS),
    );
    return;
  }
  const known = Object.values(stages).filter((f): f is number => f !== null);
  const shown = known.length ? known.reduce((a, b) => a + b, 0) / known.length : null;
  useProgress.setState({ [asset]: { stages, stage: done ? (before?.stage ?? stage) : stage, shown } });
}

/** Starts listening to the backend's progress; the answer stops it. */
export function watchMediaProgress(): Promise<() => void> {
  if (!("__TAURI_INTERNALS__" in window)) return Promise.resolve(() => undefined);
  return listen<Payload>("media-progress", ({ payload }) => update(payload));
}

/** A bar along the bottom of a clip of `asset` while the backend works on it; nothing otherwise. */
export function MediaProgressBar({ asset }: { asset: string }) {
  const t = useT();
  const step = useProgress((all) => all[asset]);
  if (!step) return null;
  const known = step.shown !== null;
  return (
    <span
      className={`media-progress${known ? "" : " unknown"}`}
      role="progressbar"
      aria-label={t(`media.progress.${step.stage}` as MessageKey)}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={known ? Math.round(step.shown! * 100) : undefined}
    >
      <span style={known ? { width: `${step.shown! * 100}%` } : undefined} />
    </span>
  );
}
