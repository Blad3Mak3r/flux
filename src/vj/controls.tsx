// The small controls of the VJ screen: a number scrubbed by its handle, a choice among a few and a colour.

import { useEffect, useState } from "react";
import type { PointerEvent, ReactNode } from "react";
import { useT } from "../i18n";
import type { VjTarget } from "../project/types";
import { mappable } from "./mapTarget";

/**
 * A number set by dragging its label sideways, as in Resolume, by typing it or with the arrow keys; a
 * double click on the label puts it back to `reset`. `value` and what it sends are in the backend's units, shown times
 * `shown` (e.g. 100 for a percent).
 */
export function Scrub({ label, name, value, shown, reset, unit = "%", decimals = 0, perPixel, step, marker, trailing, map, onMove, onSet }: {
  /** The handle's short tag. */
  label: string;
  /** What the number is, for a screen reader. */
  name: string;
  value: number;
  shown: number;
  reset: number;
  /** Shown after the number. */
  unit?: string;
  /** Places after the point it is shown with. */
  decimals?: number;
  /** How much a pixel of dragging moves it, in the backend's units; one shown unit by default. */
  perPixel?: number;
  /** How much an arrow key moves it, in the backend's units; one shown unit by default. */
  step?: number;
  /** Where a modulation has the value now, 0 to 1 along the setting's range: a mark under the number. */
  marker?: number;
  /** Something drawn at the end of the box, such as the button of its modulation. */
  trailing?: ReactNode;
  /** What this number is in the map mode: a MIDI control or an OSC address can drive it. */
  map?: VjTarget;
  onMove: (value: number) => void;
  onSet: (value: number) => void;
}) {
  const t = useT();
  const [held, setHeld] = useState<{ x: number; from: number; v: number; moved: boolean } | null>(null);
  const [text, setText] = useState<string | null>(null);
  const current = held?.v ?? value;
  const display = (current * shown).toFixed(decimals);
  // One pixel is one shown unit; with Shift a tenth of one.
  const dragged = (event: PointerEvent<HTMLElement>, from: { x: number; from: number }) =>
    from.from + (event.clientX - from.x) * (event.shiftKey ? 0.1 : 1) * (perPixel ?? 1 / shown);
  const commit = () => {
    if (text !== null && text.trim() !== "" && Number.isFinite(Number(text))) onSet(Number(text) / shown);
    setText(null);
  };
  return (
    <span className={`vj-scrub${marker === undefined ? "" : " modulated"}${trailing ? " has-trailing" : ""}`} {...mappable(map)}>
      <span
        className="vj-scrub-l"
        title={t("vj.clip.scrub")}
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId);
          setHeld({ x: event.clientX, from: value, v: value, moved: false });
        }}
        onPointerMove={(event) => {
          if (!held) return;
          const v = dragged(event, held);
          setHeld({ ...held, v, moved: true });
          onMove(v);
        }}
        onPointerUp={(event) => {
          if (!held) return;
          const v = dragged(event, held);
          setHeld(null);
          // Even back where it started: the previews moved the output, and this puts it back.
          if (held.moved) onSet(v);
        }}
        onPointerCancel={() => setHeld(null)}
        onDoubleClick={() => onSet(reset)}
      >{label}</span>
      <input className="vj-input vj-mono" inputMode="decimal" aria-label={name} value={text ?? display}
        onFocus={(event) => event.currentTarget.select()}
        onChange={(event) => setText(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") event.currentTarget.blur();
          if (event.key === "Escape") { setText(null); event.currentTarget.blur(); }
          // Arrows step one unit, ten with Shift.
          const direction = event.key === "ArrowUp" ? 1 : event.key === "ArrowDown" ? -1 : 0;
          if (direction) {
            event.preventDefault();
            setText(null);
            onSet(value + direction * (event.shiftKey ? 10 : 1) * (step ?? 1 / shown));
          }
        }} />
      <span className="vj-scrub-u">{unit}</span>
      {trailing}
      {marker !== undefined && <i className="vj-scrub-m" style={{ left: `${Math.min(1, Math.max(0, marker)) * 100}%` }} aria-hidden="true" />}
    </span>
  );
}

export function Segments<T extends string>({ options, value, label, title, onPick }: {
  options: T[];
  value: T;
  label: (option: T) => ReactNode;
  /** What a segment is called when its label is only a picture. */
  title?: (option: T) => string;
  onPick: (option: T) => void;
}) {
  return (
    <div className="vj-segs" role="radiogroup">
      {options.map((option) => (
        <button key={option} type="button" role="radio" aria-checked={option === value} className={option === value ? "on" : ""}
          title={title?.(option)} aria-label={title?.(option)}
          onClick={() => { if (option !== value) onPick(option); }}>{label(option)}</button>
      ))}
    </div>
  );
}

/** A colour picked in place: the output follows while it is picked, and it is kept when the picker closes. */
export function ColourField({ value, label, onMove, onSet }: {
  value: string;
  label: string;
  onMove: (colour: string) => void;
  onSet: (colour: string) => void;
}) {
  const [colour, setColour] = useState(value);
  useEffect(() => setColour(value), [value]);
  return (
    <label className="vj-colour" title={label}>
      <input type="color" aria-label={label} value={colour}
        onChange={(event) => { setColour(event.target.value); onMove(event.target.value); }}
        onBlur={() => { if (colour !== value) onSet(colour); }} />
      <span className="vj-mono">{colour.toUpperCase()}</span>
    </label>
  );
}
