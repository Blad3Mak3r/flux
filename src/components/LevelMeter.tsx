import { useCallback, useEffect, useRef, useState } from "react";
import {
  BALLISTICS_START,
  METER_FLOOR_DB,
  METER_SEGMENTS,
  formatDb,
  dbFromLinear,
  isSettled,
  litSegments,
  segmentOf,
  segmentTopDb,
  stepBallistics,
  toneOf,
} from "../audio/level";
import type { Ballistics } from "../audio/level";
import { useT } from "../i18n";
import { useTransport } from "../transport/store";

/** Printed on the scale. Ticks without a label sit between them. */
const LABELLED = [-48, -24, -12, 0];
const TICKS = [-36, -6, -3];
const SEGMENTS = Array.from({ length: METER_SEGMENTS }, (_, index) => index);

interface View {
  lit: number;
  hold: number;
  clip: boolean;
}

const IDLE: View = { lit: 0, hold: -1, clip: false };

function viewOf(state: Ballistics): View {
  return { lit: litSegments(state.level), hold: segmentOf(state.hold), clip: state.clip };
}

/**
 * Follows a track's peak. The loop only runs while there is something to show: it starts on any
 * new level or position, and stops once the bar and the hold marker have fallen to silence.
 */
function useMeter(trackId: string): [View, () => void] {
  const [view, setView] = useState<View>(IDLE);
  const state = useRef<Ballistics>(BALLISTICS_START);
  const wake = useRef<() => void>(() => undefined);

  useEffect(() => {
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;
    let last = 0;
    const tick = (now: number) => {
      const dt = Math.min(now - last, 100);
      last = now;
      const { position, levels } = useTransport.getState();
      const peak = position?.state === "playing" ? (levels[trackId] ?? 0) : 0;
      state.current = stepBallistics(state.current, dbFromLinear(peak), dt, reduced.matches);
      const next = viewOf(state.current);
      setView((was) =>
        was.lit === next.lit && was.hold === next.hold && was.clip === next.clip ? was : next,
      );
      frame = isSettled(state.current) && peak === 0 ? 0 : requestAnimationFrame(tick);
    };
    const start = () => {
      if (frame) return;
      last = performance.now();
      frame = requestAnimationFrame(tick);
    };
    wake.current = start;
    const unsubscribe = useTransport.subscribe(start);
    return () => {
      unsubscribe();
      cancelAnimationFrame(frame);
    };
  }, [trackId]);

  const reset = useCallback(() => {
    state.current = { ...state.current, clip: false };
    setView(viewOf(state.current));
    wake.current();
  }, []);

  return [view, reset];
}

/**
 * A segmented peak meter for one track, after its fader: green to -12 dBFS, yellow to -3, red
 * above. The peak-hold marker is a lone lit segment; clipping latches in the CLIP lamp until it is
 * pressed. Level is also in the printed scale and in the accessible value, never colour alone.
 */
export function LevelMeter({ trackId, name }: { trackId: string; name: string }) {
  const t = useT();
  const [view, reset] = useMeter(trackId);
  const bar = view.lit > 0 ? segmentTopDb(view.lit - 1) : METER_FLOOR_DB;
  return (
    <div className="meter">
      <div className="meter-body">
        <div
          className="meter-bar"
          role="meter"
          aria-label={t("track.meter", { name })}
          aria-valuemin={METER_FLOOR_DB}
          aria-valuemax={0}
          aria-valuenow={bar}
          aria-valuetext={t("track.meter.value", { value: formatDb(bar) })}
        >
          {SEGMENTS.map((index) => (
            <span
              key={index}
              className="meter-segment"
              data-tone={toneOf(segmentTopDb(index))}
              data-on={index < view.lit || index === view.hold}
              data-hold={index === view.hold}
            />
          ))}
        </div>
        <div className="meter-scale" aria-hidden="true">
          {[...TICKS, ...LABELLED].map((db) => (
            <span
              key={db}
              className="meter-tick"
              data-end={db === 0}
              style={{ left: `${((db - METER_FLOOR_DB) / -METER_FLOOR_DB) * 100}%` }}
            >
              {LABELLED.includes(db) ? db : ""}
            </span>
          ))}
        </div>
      </div>
      <button
        type="button"
        className="meter-clip"
        aria-pressed={view.clip}
        aria-label={t("track.meter.reset", { name })}
        title={t("track.meter.reset", { name })}
        onClick={reset}
      >
        {t("track.meter.clip")}
      </button>
    </div>
  );
}
