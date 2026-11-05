import { useState } from "react";
import { useT } from "../i18n";
import { MAX_BEATS_PER_BAR, MAX_BPM, MIN_BPM } from "../project/types";
import { setTimelineTempo } from "../state/actions";
import { useDoc, useTimeline } from "../state/store";
import { openSetup, setRulerMode, useUi } from "../state/ui";
import type { RulerMode } from "../state/ui";

const BEAT_COUNTS = Array.from({ length: MAX_BEATS_PER_BAR }, (_, i) => i + 1);

/** The tempo as a number to type: applied on Enter or blur, refused with a note when out of range. */
function BpmInput({ bpm }: { bpm: number }) {
  const t = useT();
  const [draft, setDraft] = useState<string | null>(null);
  const [invalid, setInvalid] = useState(false);

  const commit = () => {
    if (draft === null) return;
    const value = Number(draft.replace(",", "."));
    if (draft.trim() === "" || !Number.isFinite(value) || value < MIN_BPM || value > MAX_BPM) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setDraft(null);
    setTimelineTempo({ bpm: value });
  };

  return (
    <input
      type="text"
      className="bpm-input mono"
      inputMode="decimal"
      aria-label={t("tempo.bpm")}
      aria-invalid={invalid || undefined}
      title={invalid ? t("tempo.bpm.invalid", { min: MIN_BPM, max: MAX_BPM }) : t("tempo.bpm")}
      value={draft ?? String(bpm)}
      spellCheck={false}
      autoComplete="off"
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit();
        if (event.key === "Escape") {
          setDraft(null);
          setInvalid(false);
          event.currentTarget.blur();
        }
      }}
    />
  );
}

/**
 * What the ruler counts (timecode, or bars and beats), the timeline's tempo and its metronome
 * switch. The ruler choice is a way of looking; the tempo and the click belong to the timeline.
 */
export function TempoBar() {
  const t = useT();
  const { tempo } = useTimeline();
  const ruler = useUi((ui) => ui.ruler);
  const hasOutput = useDoc((doc) => doc.project.output.metronome !== undefined);
  const clicking = tempo.click && hasOutput;
  return (
    <div className="tempo-bar" role="group" aria-label={t("tempo.legend")}>
      <select
        className="ruler-select"
        aria-label={t("tempo.ruler")}
        value={ruler}
        onChange={(event) => setRulerMode(event.target.value as RulerMode)}
      >
        <option value="time">{t("tempo.ruler.time")}</option>
        <option value="bars">{t("tempo.ruler.bars")}</option>
      </select>
      <BpmInput bpm={tempo.bpm} />
      <span className="tempo-unit" aria-hidden="true">
        BPM
      </span>
      <select
        className="beats-select"
        aria-label={t("tempo.beats")}
        title={t("tempo.beats")}
        value={tempo.beatsPerBar}
        onChange={(event) => setTimelineTempo({ beatsPerBar: Number(event.target.value) })}
      >
        {BEAT_COUNTS.map((beats) => (
          <option key={beats} value={beats}>
            {beats}/4
          </option>
        ))}
      </select>
      <button
        type="button"
        className="follow-toggle click-toggle"
        aria-pressed={clicking}
        title={hasOutput ? t("tempo.click.hint") : t("tempo.click.setup")}
        onClick={() => (hasOutput ? setTimelineTempo({ click: !tempo.click }) : openSetup("metronome"))}
      >
        <span className="lamp" data-state={clicking ? "on" : "off"} aria-hidden="true" />
        {t("tempo.click")}
      </button>
    </div>
  );
}
