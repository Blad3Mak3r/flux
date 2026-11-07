import { useEffect, useRef } from "react";
import type { KeyboardEvent } from "react";

/**
 * A name typed where it is shown. Enter or leaving the field keeps it, Esc leaves it as it was.
 * The field keeps every pointer and key event to itself, so typing never drags or nudges the clip
 * it sits in, nor fires a transport key.
 */
export function InlineName({
  value,
  label,
  onCommit,
  onDone,
}: {
  value: string;
  /** What the field is for, read by assistive technology. */
  label: string;
  /** Called with the typed text, unless it is unchanged. */
  onCommit: (text: string) => void;
  /** Called once when the field closes, however it closed. */
  onDone: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const closed = useRef(false);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  const close = (keep: boolean) => {
    if (closed.current) return;
    closed.current = true;
    const text = input.current?.value ?? value;
    if (keep && text !== value) onCommit(text);
    onDone();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    event.stopPropagation();
    if (event.key === "Enter") {
      event.preventDefault();
      close(true);
    } else if (event.key === "Escape") {
      event.preventDefault();
      close(false);
    }
  };

  return (
    <input
      ref={input}
      type="text"
      className="inline-name"
      defaultValue={value}
      aria-label={label}
      spellCheck={false}
      onKeyDown={onKeyDown}
      onBlur={() => close(true)}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => event.stopPropagation()}
      onDoubleClick={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.stopPropagation()}
    />
  );
}
