// The keyboard as a mapping source. The window only forwards what was pressed (the key's place, its modifiers, down or
// up); the backend decides whether a mapping answers it, and refuses to learn a key the app uses itself
// (`Source::is_app_shortcut`). Nothing here parses or matches.

import { controlKey } from "../control/api";
import { useDoc } from "../state/store";
import { useMap } from "./mapping";

const MODIFIERS = /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/;

/** The modifiers each key was pressed with, so its release says the same and a Hold mapping lets go. */
const down = new Map<string, { ctrl: boolean; alt: boolean; shift: boolean; meta: boolean }>();

const send = (code: string, mods: { ctrl: boolean; alt: boolean; shift: boolean; meta: boolean }, isDown: boolean) =>
  void controlKey({ code, ...mods, down: isDown }).catch(() => undefined);

/**
 * A key went down in a VJ show with nothing being typed and no dialog open. `true` when it was taken (only while a
 * mapping waits for a key): the app's own shortcuts must not also run. Esc is never taken, so it still cancels. */
export function forwardKeyDown(event: KeyboardEvent): boolean {
  if (event.repeat || event.code === "Escape" || MODIFIERS.test(event.code)) return false;
  const mods = { ctrl: event.ctrlKey, alt: event.altKey, shift: event.shiftKey, meta: event.metaKey };
  const learning = useMap.getState().learn !== null;
  // Every key goes to the backend while learning; it refuses the few the app uses (`shortcuts.ts`).
  if (learning) {
    down.set(event.code, mods);
    send(event.code, mods, true);
    return true;
  }
  const grid = useDoc.getState().project.vj;
  // In map mode the keys edit; they do not play.
  if (!useMap.getState().on && grid?.mappings?.some((mapping) => mapping.source.type === "key")) {
    down.set(event.code, mods);
    send(event.code, mods, true);
  }
  return false;
}

/** A key came up: every key that went down is let go, wherever the focus is by now. */
export function forwardKeyUp(event: KeyboardEvent): void {
  const mods = down.get(event.code);
  if (!mods) return;
  down.delete(event.code);
  send(event.code, mods, false);
}

/** The window lost the focus: no key is held any more. */
export function releaseKeys(): void {
  for (const [code, mods] of down) send(code, mods, false);
  down.clear();
}
