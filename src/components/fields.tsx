import { useId, useState } from "react";
import type { ReactNode } from "react";
import { useT } from "../i18n";
import type { FrameRate } from "../project/types";
import { formatFrames, parseTimecode } from "../timecode";

/** A labelled control. The label is a real <label>, so clicking it focuses the input. */
export function Field({
  label,
  hint,
  error,
  children,
}: {
  label: string;
  hint?: string;
  error?: string | null;
  children: (props: { id: string; describedBy: string | undefined }) => ReactNode;
}) {
  const id = useId();
  const note = error ?? hint;
  return (
    <div className="field" data-invalid={error ? true : undefined}>
      <label htmlFor={id}>{label}</label>
      {children({ id, describedBy: note ? `${id}-note` : undefined })}
      {note && (
        <p id={`${id}-note`} className="field-note" role={error ? "alert" : undefined}>
          {note}
        </p>
      )}
    </div>
  );
}

/** Text that is only applied on Enter or blur, so typing never makes half-valid history entries. */
export function TextField({
  label,
  value,
  onCommit,
  validate,
  error,
  hint,
  inputMode,
  monospace,
}: {
  label: string;
  value: string;
  onCommit: (value: string) => void;
  /** Return an error message to refuse the value. */
  validate?: (value: string) => string | null;
  error?: string | null;
  hint?: string;
  inputMode?: "text" | "numeric" | "decimal";
  monospace?: boolean;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  const commit = () => {
    if (draft === null) return;
    const refusal = validate?.(draft) ?? null;
    if (refusal) {
      setProblem(refusal);
      return;
    }
    setProblem(null);
    setDraft(null);
    if (draft !== value) onCommit(draft);
  };

  return (
    <Field label={label} hint={hint} error={problem ?? error ?? null}>
      {({ id, describedBy }) => (
        <input
          id={id}
          type="text"
          className={monospace ? "mono" : undefined}
          inputMode={inputMode}
          value={draft ?? value}
          aria-invalid={problem || error ? true : undefined}
          aria-describedby={describedBy}
          spellCheck={false}
          autoComplete="off"
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
            if (event.key === "Escape") {
              setDraft(null);
              setProblem(null);
            }
          }}
        />
      )}
    </Field>
  );
}

/**
 * Several lines of text, applied when focus leaves it: Enter writes a new line, so it cannot be
 * the key that commits.
 */
export function TextAreaField({
  label,
  value,
  onCommit,
  hint,
  rows = 5,
  autoFocus,
}: {
  label: string;
  value: string;
  onCommit: (value: string) => void;
  hint?: string;
  rows?: number;
  autoFocus?: boolean;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft !== null && draft !== value) onCommit(draft);
    setDraft(null);
  };
  return (
    <Field label={label} hint={hint}>
      {({ id, describedBy }) => (
        <textarea
          id={id}
          aria-describedby={describedBy}
          className="textarea"
          rows={rows}
          autoFocus={autoFocus}
          value={draft ?? value}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              setDraft(null);
              event.currentTarget.blur();
            }
          }}
        />
      )}
    </Field>
  );
}

/** A timecode shown with the project's start offset; edits are parsed at the project frame rate. */
export function TimecodeField({
  label,
  frame,
  rate,
  onCommit,
  hint,
}: {
  label: string;
  /** Absolute frame count, including the start offset. */
  frame: number;
  rate: FrameRate;
  onCommit: (frame: number) => void;
  hint?: string;
}) {
  const t = useT();
  return (
    <TextField
      label={label}
      value={formatFrames(frame, rate)}
      hint={hint}
      monospace
      inputMode="numeric"
      validate={(text) => (parseTimecode(text, rate) === null ? t("field.timecodeInvalid") : null)}
      onCommit={(text) => {
        const parsed = parseTimecode(text, rate);
        if (parsed !== null) onCommit(parsed);
      }}
    />
  );
}

export function SelectField({
  label,
  value,
  onChange,
  children,
  hint,
  disabled,
}: {
  label: string;
  value: string | number;
  onChange: (value: string) => void;
  children: ReactNode;
  hint?: string;
  disabled?: boolean;
}) {
  return (
    <Field label={label} hint={hint}>
      {({ id, describedBy }) => (
        <select
          id={id}
          value={value}
          disabled={disabled}
          aria-describedby={describedBy}
          onChange={(event) => onChange(event.target.value)}
        >
          {children}
        </select>
      )}
    </Field>
  );
}
