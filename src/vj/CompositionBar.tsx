// The composition bar of the VJ screen, as in Resolume: the show's tempo and its timecode clock.
// The backend keeps the tempo and the clock's source; the transport runs the clock.

import { useEffect, useRef, useState } from "react";
import type { FocusEvent } from "react";
import { PauseIcon, PlayIcon, StopIcon } from "../components/icons";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { VjClockSource, VjGrid } from "../project/types";
import { activeTimeline } from "../state/doc";
import { useDoc } from "../state/store";
import { openSetup } from "../state/ui";
import { formatFrames, parseTimecode } from "../timecode";
import { pause, play, seek, stop } from "../transport/playback";
import { selectFrame, selectMode, useTransport } from "../transport/store";
import { vjAnalysisLevels } from "./api";
import type { AnalysisLevels } from "./api";
import { edit } from "./store";
import { closeTimecodeIn, useTimecodeIn } from "./timecodeIn";
import type { TimecodeInStatus } from "./timecodeIn";

/** Selects a field's text once the click that focused it is over, so typing replaces it. */
const selectAll = (event: FocusEvent<HTMLInputElement>) => {
  const field = event.currentTarget;
  requestAnimationFrame(() => field.select());
};

const SOURCES: VjClockSource[] = ["internal", "ltc", "mtc"];
/** The taps the backend reads the tempo from: the last ones are enough. */
const TAPS = 8;

/** The show's tempo: typed, nudged, halved, doubled or tapped. */
export function Tempo() {
  const t = useT();
  const bpm = useDoc((doc) => activeTimeline(doc).tempo.bpm);
  const [draft, setDraft] = useState<string | null>(null);
  const taps = useRef<number[]>([]);
  const commit = () => {
    if (draft !== null && draft.trim() !== "") void edit({ op: "setBpm", bpm: Number(draft.replace(",", ".")) });
    setDraft(null);
  };
  const tap = () => {
    taps.current = [...taps.current, performance.now()].slice(-TAPS);
    if (taps.current.length > 1) void edit({ op: "tapBpm", taps: taps.current });
  };
  return (
    <div className="vj-bpm" role="group" aria-label={t("vj.tempo.label")}>
      <span className="vj-lbl">BPM</span>
      <input
        className="vj-bpm-v"
        aria-label={t("vj.tempo.label")}
        inputMode="decimal"
        value={draft ?? bpm.toFixed(2)}
        onFocus={selectAll}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
          if (event.key === "Escape") {
            setDraft(null);
            event.currentTarget.blur();
          }
        }}
      />
      <div className="vj-bpm-k">
        <button type="button" title={t("vj.tempo.down")} aria-label={t("vj.tempo.down")} onClick={() => void edit({ op: "nudgeBpm", by: -1 })}>−</button>
        <button type="button" title={t("vj.tempo.up")} aria-label={t("vj.tempo.up")} onClick={() => void edit({ op: "nudgeBpm", by: 1 })}>+</button>
        <button type="button" title={t("vj.tempo.half")} aria-label={t("vj.tempo.half")} onClick={() => void edit({ op: "scaleBpm", factor: 0.5 })}>÷2</button>
        <button type="button" title={t("vj.tempo.double")} aria-label={t("vj.tempo.double")} onClick={() => void edit({ op: "scaleBpm", factor: 2 })}>×2</button>
      </div>
      <button type="button" className="vj-tap" title={t("vj.tempo.tapNote")} onPointerDown={tap}>
        {t("vj.tempo.tap")}
      </button>
    </div>
  );
}

/**
 * The show's timecode. On the internal clock it is Flux's transport: played, paused and taken back
 * to the start here, and typed while it does not run. From LTC or MTC it shows what comes in, which
 * the backend reads from the input chosen in Preferences, and nothing here moves it.
 */
export function TimecodeClock({ grid }: { grid: VjGrid }) {
  const t = useT();
  const source = grid.clock ?? "internal";
  const internal = source === "internal";
  const { frameRate } = useDoc((doc) => activeTimeline(doc).timecode);
  const frame = useTransport(selectFrame);
  const state = useTransport(selectMode);
  const [draft, setDraft] = useState<string | null>(null);
  const incoming = useTimecodeIn(internal ? null : source);
  const reading = incoming?.reading ?? null;
  // The input stays open while the clock shows it, and closes when it goes back to Flux's own.
  useEffect(() => {
    if (!internal) return () => void closeTimecodeIn();
  }, [internal]);
  const playing = state === "playing";
  const editable = internal && !playing;
  const bad = draft !== null && parseTimecode(draft, frameRate) === null;
  const commit = () => {
    const target = draft === null ? null : parseTimecode(draft, frameRate);
    if (target !== null) seek(target);
    setDraft(null);
  };
  const shown = internal ? formatFrames(frame, frameRate) : reading ? formatFrames(reading.frame, reading.rate) : "--:--:--:--";
  const signal = signalOf(incoming);
  const sourceName = t(`vj.clock.short.${source}` as MessageKey);
  return (
    <div className="vj-tc" role="group" aria-label={t("vj.clock.label")} data-state={internal ? state : "external"} data-signal={internal ? undefined : signal}>
      <div className="vj-segs vj-tc-src" role="radiogroup" aria-label={t("vj.clock.source")}>
        {SOURCES.map((s) => (
          <button
            key={s}
            type="button"
            role="radio"
            aria-checked={s === source}
            className={s === source ? "on" : undefined}
            title={t(`vj.clock.${s}` as MessageKey)}
            onClick={() => s !== source && void edit({ op: "setClock", source: s })}
          >
            {t(`vj.clock.short.${s}` as MessageKey)}
          </button>
        ))}
      </div>
      {editable ? (
        <input
          className="vj-tc-v"
          aria-label={t("vj.clock.edit")}
          title={t("vj.clock.edit")}
          aria-invalid={bad || undefined}
          spellCheck={false}
          value={draft ?? shown}
          onFocus={selectAll}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
            if (event.key === "Escape") {
              setDraft(null);
              event.currentTarget.blur();
            }
          }}
        />
      ) : (
        <output className="vj-tc-v" title={internal ? undefined : t("vj.clock.externalNote", { source: sourceName })}>
          {shown}
        </output>
      )}
      <span className="vj-tc-rate">{t(`frameRate.${reading?.rate ?? frameRate}` as MessageKey)}</span>
      {internal ? (
        <div className="vj-tc-keys">
          <button type="button" className={playing ? "on" : undefined} title={t(playing ? "vj.clock.pause" : "vj.clock.play")}
            aria-label={t(playing ? "vj.clock.pause" : "vj.clock.play")} onClick={() => void (playing ? pause() : play())}>
            {playing ? <PauseIcon /> : <PlayIcon />}
          </button>
          <button type="button" title={t("vj.clock.stop")} aria-label={t("vj.clock.stop")} onClick={() => void stop()}>
            <StopIcon />
          </button>
        </div>
      ) : (
        <button
          type="button"
          className="vj-tc-in"
          data-signal={signal}
          title={`${t(`vj.clock.signal.${signal}` as MessageKey, { source: sourceName })}\n${t("vj.clock.chooseInput")}`}
          onClick={() => openSetup("timecodeIn")}
        >
          <span className="lamp" data-state={signal === "running" ? "on" : signal === "missing" || signal === "error" ? "fault" : "off"} aria-hidden="true" />
          {t("vj.clock.in", { source: sourceName })}
        </button>
      )}
    </div>
  );
}

/** How the incoming timecode is doing, in one word for the lamp and its title. */
type Signal = "unset" | "missing" | "error" | "waiting" | "stopped" | "running";

function signalOf(status: TimecodeInStatus | null): Signal {
  if (!status) return "waiting";
  if (status.state !== "listening") return status.state;
  if (!status.reading) return "waiting";
  return status.reading.running ? "running" : "stopped";
}

/** The bands the analysis hears, as the backend reads them. */
const BANDS = ["low", "mid", "high", "level"] as const;
/** How often the meter asks: fifteen times a second is enough to see it move. */
const METER_EVERY = 1000 / 15;

/**
 * The audio analysis meter of the composition bar: the low, mid and high bands and the level that audio modulation
 * follows. It only draws what the backend reads; a click opens the page that chooses what is listened to.
 */
export function AnalysisMeter() {
  const t = useT();
  const [levels, setLevels] = useState<AnalysisLevels | null>(null);
  useEffect(() => {
    let stopped = false;
    const tick = () => {
      if (stopped) return;
      if (document.hidden) {
        window.setTimeout(tick, 200);
        return;
      }
      vjAnalysisLevels()
        .then((next) => !stopped && setLevels(next))
        .catch(() => undefined)
        .finally(() => !stopped && window.setTimeout(tick, METER_EVERY));
    };
    tick();
    return () => {
      stopped = true;
    };
  }, []);
  const state = levels?.state ?? "off";
  const word = t(`setup.analysis.state.${state}` as MessageKey);
  return (
    <button type="button" className="vj-meter" data-state={state} title={t("vj.mod.meterOpen", { state: word })} aria-label={t("vj.mod.meterOpen", { state: word })}
      onClick={() => openSetup("analysis")}>
      {BANDS.map((band) => (
        <span key={band} className="vj-meter-b" data-band={band} title={t(`vj.mod.band.${band}` as MessageKey)}>
          <i style={{ height: `${Math.round((levels?.[band] ?? 0) * 100)}%` }} />
          <b aria-hidden="true">{t(`vj.mod.meter.${band}` as MessageKey)}</b>
        </span>
      ))}
    </button>
  );
}
