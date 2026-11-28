// The composition bar of the VJ screen, as in Resolume: the show's tempo and its timecode clock.
// The backend keeps the tempo and the clock's source; the transport runs the clock.

import { pad2, parseDecimal } from "../format";
import { useEffect, useRef, useState } from "react";
import type { FocusEvent } from "react";
import { PauseIcon, PlayIcon, StopIcon } from "../components/icons";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { VjClockSource, VjGrid } from "../project/types";
import { activeTimeline } from "../state/doc";
import { useDoc } from "../state/store";
import { openSetup } from "../state/ui";
import { parseTimecode } from "../project/api";
import { formatFrames } from "../timecode";
import { pause, play, seek, stop } from "../transport/playback";
import { selectFrame, selectMode, useTransport } from "../transport/store";
import { vjAnalysisLevels } from "./api";
import { stopRecording, useRecord } from "./record";
import { mappable } from "./mapModel";
import type { AnalysisLevels } from "./api";
import { beatClock, edit, useVj } from "./store";
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

/** The beat in its bar of four and a square that flashes on each beat; dim while the beats are held. Its beat is the backend's count. */
function BeatLights() {
  const beat = useVj((vj) => Math.floor(vj.play.beats));
  const paused = useVj((vj) => vj.play.beatsPaused);
  const t = useT();
  const inBar = ((beat % 4) + 4) % 4;
  return (
    <div className={`vj-beat${paused ? " held" : ""}`} role="img" aria-label={t("vj.beat.label", { n: inBar + 1 })}>
      <i key={beat} className="vj-beat-flash" />
      {[0, 1, 2, 3].map((n) => <i key={n} className={`vj-beat-n${n === inBar ? " on" : ""}`} />)}
    </div>
  );
}

/** A key that holds the beats a little faster or slower for as long as it is held, by pointer or keyboard. */
function NudgeKey({ direction, label, children }: { direction: -1 | 1; label: string; children: string }) {
  const map = { kind: direction > 0 ? "bpmNudgeUp" : "bpmNudgeDown" } as const;
  const release = () => void beatClock({ op: "nudge", direction: 0 });
  return (
    <button type="button" {...mappable(map)} title={label} aria-label={label}
      onPointerDown={(event) => { event.currentTarget.setPointerCapture(event.pointerId); void beatClock({ op: "nudge", direction }); }}
      onPointerUp={release}
      onPointerCancel={release}
      onKeyDown={(event) => { if ((event.key === "Enter" || event.key === " ") && !event.repeat) void beatClock({ op: "nudge", direction }); }}
      onKeyUp={(event) => { if (event.key === "Enter" || event.key === " ") release(); }}
      onBlur={release}>
      {children}
    </button>
  );
}

/** The show's tempo: typed, nudged, halved, doubled or tapped; its beats paused, resynced or nudged. */
export function Tempo() {
  const t = useT();
  const bpm = useDoc((doc) => activeTimeline(doc).tempo.bpm);
  const [draft, setDraft] = useState<string | null>(null);
  const paused = useVj((vj) => vj.play.beatsPaused);
  const taps = useRef<number[]>([]);
  const commit = () => {
    if (draft !== null && draft.trim() !== "") void edit({ op: "setBpm", bpm: parseDecimal(draft) });
    setDraft(null);
  };
  const tap = () => {
    taps.current = [...taps.current, performance.now()].slice(-TAPS);
    if (taps.current.length > 1) void edit({ op: "tapBpm", taps: taps.current });
  };
  return (
    <div className="vj-bpm" role="group" aria-label={t("vj.tempo.label")}>
      <BeatLights />
      <span className="vj-lbl">BPM</span>
      <input
        className="vj-bpm-v"
        {...mappable({ kind: "bpm" })}
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
        <button type="button" {...mappable({ kind: "bpmMinus" })} title={t("vj.tempo.down")} aria-label={t("vj.tempo.down")} onClick={() => void edit({ op: "nudgeBpm", by: -1 })}>−</button>
        <button type="button" {...mappable({ kind: "bpmPlus" })} title={t("vj.tempo.up")} aria-label={t("vj.tempo.up")} onClick={() => void edit({ op: "nudgeBpm", by: 1 })}>+</button>
        <button type="button" {...mappable({ kind: "bpmHalf" })} title={t("vj.tempo.half")} aria-label={t("vj.tempo.half")} onClick={() => void edit({ op: "scaleBpm", factor: 0.5 })}>÷2</button>
        <button type="button" {...mappable({ kind: "bpmDouble" })} title={t("vj.tempo.double")} aria-label={t("vj.tempo.double")} onClick={() => void edit({ op: "scaleBpm", factor: 2 })}>×2</button>
      </div>
      <div className="vj-bpm-k">
        <NudgeKey direction={-1} label={t("vj.beat.nudgeBack")}>|◀</NudgeKey>
        <NudgeKey direction={1} label={t("vj.beat.nudgeForward")}>▶|</NudgeKey>
      </div>
      <button type="button" className="vj-tap" {...mappable({ kind: "bpmTap" })} title={t("vj.tempo.tapNote")} onPointerDown={tap}>
        {t("vj.tempo.tap")}
      </button>
      <button type="button" className="vj-tap" {...mappable({ kind: "bpmResync" })} title={t("vj.beat.resyncNote")} onClick={() => void beatClock({ op: "resync" })}>
        {t("vj.beat.resync")}
      </button>
      <button type="button" className="vj-tap vj-hold" {...mappable({ kind: "bpmPause" })} aria-pressed={paused} title={t("vj.beat.pauseNote")} onClick={() => void beatClock({ op: "pause", on: !paused })}>
        {t("vj.beat.pause")}
      </button>
      <LinkToggle />
      <RecordIndicator />
    </div>
  );
}

/** Joins or leaves the Ableton Link session, with the number of other applications in it while joined. The backend keeps the session. */
function LinkToggle() {
  const t = useT();
  const on = useDoc((doc) => doc.project.vj?.link ?? false);
  const peers = useVj((vj) => vj.play.linkPeers);
  const title = on ? t("vj.link.on", { peers: peers ?? 0 }) : t("vj.link.off");
  return (
    <button type="button" className="vj-tap vj-hold vj-link" aria-pressed={on} title={title} aria-label={t("vj.link.label")}
      onClick={() => void edit({ op: "setLink", on: !on })}>
      {t("vj.link.name")}
      {on && <b className="vj-link-n" title={t("vj.link.peers", { n: peers ?? 0 })}>{peers ?? 0}</b>}
    </button>
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
  const commit = () => {
    const text = draft;
    setDraft(null);
    if (text !== null) void parseTimecode(text, frameRate).then((target) => target !== null && seek(target));
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

/** Elapsed milliseconds as `H:MM:SS`. */
const elapsed = (ms: number) => {
  const total = Math.floor(ms / 1000);
  return `${Math.floor(total / 3600)}:${pad2(Math.floor(total / 60) % 60)}:${pad2(total % 60)}`;
};

/** Shown while the composition is being recorded: the word REC beside its dot, how long, the frames that were late, and the key that stops it. */
function RecordIndicator() {
  const t = useT();
  const recording = useRecord((r) => r.recording);
  const elapsedMs = useRecord((r) => r.elapsedMs);
  const dropped = useRecord((r) => r.dropped);
  const path = useRecord((r) => r.path);
  if (!recording) return null;
  return (
    <div className="vj-rec" role="group" aria-label={t("vj.record.label")} title={path ?? undefined}>
      <span className="vj-rec-dot" aria-hidden="true" />
      <b>{t("vj.record.rec")}</b>
      <output className="vj-rec-time">{elapsed(elapsedMs)}</output>
      {dropped > 0 && <span className="vj-rec-drop" title={t("vj.record.droppedNote")}>{t("vj.record.dropped", { n: dropped })}</span>}
      <button type="button" title={t("vj.menu.recordStop")} aria-label={t("vj.menu.recordStop")} onClick={() => void stopRecording()}>
        <StopIcon />
      </button>
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
