import { parseDecimal } from "../format";
import { useState } from "react";
import type { KeyboardEvent } from "react";
import { Select } from "../components/Select";
import { useT } from "../i18n";
import { BEAT_UNITS, MAX_BEATS_PER_BAR, MAX_BPM, MIN_BPM } from "../project/types";
import type { TempoChange } from "../project/types";
import { deleteSelectedMark, editMarks, selectMark } from "../state/marks";

const BEAT_COUNTS = Array.from({ length: MAX_BEATS_PER_BAR }, (_, i) => i + 1);

/**
 * The tempo and the signature a change starts: opened under the change when it is picked. The tempo
 * is typed (Enter or leaving the field applies it, a value out of range is refused with a note), the
 * signature is chosen; each is one edit of the backend, which keeps the change on its bar.
 */
export function TempoChangeEditor({ change }: { change: TempoChange }) {
  const t = useT();
  const [draft, setDraft] = useState<string | null>(null);
  const [invalid, setInvalid] = useState(false);
  const unit = change.beatUnit ?? 4;
  const set = (patch: Partial<Pick<TempoChange, "bpm" | "beatsPerBar" | "beatUnit">>) =>
    void editMarks({ op: "setTempoAt", at: change.at, bpm: change.bpm, beatsPerBar: change.beatsPerBar, beatUnit: unit, ...patch });

  const commit = () => {
    if (draft === null) return;
    const value = parseDecimal(draft);
    if (draft.trim() === "" || !Number.isFinite(value) || value < MIN_BPM || value > MAX_BPM) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setDraft(null);
    if (value !== change.bpm) set({ bpm: value });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    event.stopPropagation();
    if (event.key === "Enter") commit();
    if (event.key === "Escape") {
      setDraft(null);
      setInvalid(false);
      event.currentTarget.blur();
    }
  };

  return (
    <div
      className="tempo-editor"
      role="group"
      aria-label={t("marks.tempo.edit")}
      onPointerDown={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.stopPropagation()}
    >
      <input
        type="text"
        className="bpm-input mono"
        inputMode="decimal"
        aria-label={t("tempo.bpm")}
        aria-invalid={invalid || undefined}
        title={invalid ? t("tempo.bpm.invalid", { min: MIN_BPM, max: MAX_BPM }) : t("tempo.bpm")}
        value={draft ?? String(change.bpm)}
        spellCheck={false}
        autoComplete="off"
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={onKeyDown}
      />
      <span className="tempo-unit" aria-hidden="true">
        BPM
      </span>
      <Select className="beats-select" aria-label={t("tempo.beats")} value={change.beatsPerBar} onChange={(value) => set({ beatsPerBar: Number(value) })}>
        {BEAT_COUNTS.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </Select>
      <span className="tempo-unit" aria-hidden="true">
        /
      </span>
      <Select className="beats-select" aria-label={t("marks.tempo.unit")} value={unit} onChange={(value) => set({ beatUnit: Number(value) })}>
        {BEAT_UNITS.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </Select>
      <button type="button" className="button" onClick={deleteSelectedMark}>
        {t("marks.delete")}
      </button>
      <button type="button" className="button" onClick={() => selectMark(null)}>
        {t("marks.tempo.close")}
      </button>
    </div>
  );
}
