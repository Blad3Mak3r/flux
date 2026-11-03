import { useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import { FADER_MAX_DB, FADER_MIN_DB, clampFaderDb, dbToFader, faderToDb, formatDb } from "../audio/level";
import { useT } from "../i18n";

/** Arrow keys move the fader this much; Shift makes it fine, Page keys coarse. */
const STEP_DB = 1;
const FINE_DB = 0.1;
const COARSE_DB = 6;

/**
 * A track's volume: a horizontal fader that reads as a slider to assistive technology. While it
 * is dragged `onPreview` follows every move, so a running output can be heard changing; `onCommit`
 * fires once when the gesture ends, so undo steps back a whole drag.
 */
export function Fader({
  name,
  valueDb,
  onPreview,
  onCommit,
}: {
  name: string;
  valueDb: number;
  onPreview: (db: number) => void;
  onCommit: (db: number) => void;
}) {
  const t = useT();
  const track = useRef<HTMLDivElement>(null);
  const [draft, setDraft] = useState<number | null>(null);
  const shown = draft ?? valueDb;

  const fromPointer = (event: PointerEvent) => {
    const rect = track.current?.getBoundingClientRect();
    if (!rect || rect.width === 0) return shown;
    return faderToDb((event.clientX - rect.left) / rect.width);
  };

  const move = (db: number) => {
    setDraft(db);
    onPreview(db);
  };

  const commit = (db: number) => {
    setDraft(null);
    if (db !== valueDb) onCommit(db);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const step = event.shiftKey ? FINE_DB : STEP_DB;
    const next =
      event.key === "ArrowRight" || event.key === "ArrowUp"
        ? shown + step
        : event.key === "ArrowLeft" || event.key === "ArrowDown"
          ? shown - step
          : event.key === "PageUp"
            ? shown + COARSE_DB
            : event.key === "PageDown"
              ? shown - COARSE_DB
              : event.key === "Home"
                ? FADER_MIN_DB
                : event.key === "End"
                  ? FADER_MAX_DB
                  : null;
    if (next === null) return;
    // The window seeks and plays on arrow keys; here they belong to the fader.
    event.preventDefault();
    event.stopPropagation();
    const db = clampFaderDb(next);
    onPreview(db);
    if (db !== valueDb) onCommit(db);
  };

  const label = t("track.gain", { name });
  const value = t("track.gain.value", { value: formatDb(shown) });
  return (
    <div className="fader">
      <div
        ref={track}
        className="fader-track"
        role="slider"
        tabIndex={0}
        aria-label={label}
        aria-orientation="horizontal"
        aria-valuemin={FADER_MIN_DB}
        aria-valuemax={FADER_MAX_DB}
        aria-valuenow={shown}
        aria-valuetext={value}
        title={label}
        onKeyDown={onKeyDown}
        onDoubleClick={() => {
          move(0);
          commit(0);
        }}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          move(fromPointer(event));
        }}
        onPointerMove={(event) => {
          if (draft !== null) move(fromPointer(event));
        }}
        onPointerUp={(event) => {
          if (draft !== null) commit(fromPointer(event));
        }}
        onPointerCancel={() => setDraft(null)}
      >
        <span className="fader-unity" style={{ left: `${dbToFader(0) * 100}%` }} aria-hidden="true" />
        <span className="fader-thumb" style={{ left: `${dbToFader(shown) * 100}%` }} aria-hidden="true" />
      </div>
      <span className="fader-value" aria-hidden="true">
        {formatDb(shown)}
      </span>
    </div>
  );
}
