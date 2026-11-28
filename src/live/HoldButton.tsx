import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";

/** How long the button is held before it acts: long enough that it cannot be hit by accident. */
const HOLD_MS = 700;

interface HoldButtonProps {
  className?: string;
  title: string;
  /** Called once the button has been held for long enough. */
  onHold: () => void;
  children: ReactNode;
}

/**
 * A button that acts only when it is pressed and held (the pointer, or Space and Enter), for what must
 * not happen by accident on stage. A bar fills while it is held; letting go early cancels.
 */
export function HoldButton({ className = "button", title, onHold, children }: HoldButtonProps) {
  const [holding, setHolding] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  const cancel = () => {
    window.clearTimeout(timer.current);
    timer.current = undefined;
    setHolding(false);
  };
  const begin = () => {
    if (timer.current !== undefined) return;
    setHolding(true);
    timer.current = window.setTimeout(() => {
      timer.current = undefined;
      setHolding(false);
      onHold();
    }, HOLD_MS);
  };
  useEffect(() => cancel, []);

  const keyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== " " && event.key !== "Enter") return;
    event.preventDefault();
    begin();
  };
  const keyUp = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === " " || event.key === "Enter") cancel();
  };

  return (
    <button
      type="button"
      className={`${className} hold-button`}
      data-holding={holding}
      title={title}
      onPointerDown={begin}
      onPointerUp={cancel}
      onPointerLeave={cancel}
      onPointerCancel={cancel}
      onBlur={cancel}
      onKeyDown={keyDown}
      onKeyUp={keyUp}
    >
      {children}
    </button>
  );
}
