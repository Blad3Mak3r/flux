// The window's side of the keyboard: it writes a key press as the backend's text for it, looks that up in the
// keys the backend sent, and spells a combination for a menu or the shortcuts page. Which keys do what is the
// backend's table (flux-project/src/keymap.rs); nothing here decides it.

import { usePreferences } from "../preferences/store";
import type { Action } from "./actions";

export interface ActionView {
  id: Action;
  group: "transport" | "file" | "edit" | "timeline" | "view";
  keys: string[];
  defaults: string[];
  /** Refused while the show is locked. */
  locked: boolean;
  /** Only in the editor. */
  editOnly: boolean;
}

const MAC = /Mac|iPhone|iPad/.test(navigator.platform);
const NOT_KEYS = ["Control", "Meta", "Alt", "Shift", "AltGraph", "CapsLock", "Dead", "Unidentified"];

/** The backend's text for a key press (`Mod+Shift+S`), or null for a modifier on its own. */
export function comboOf(event: KeyboardEvent): string | null {
  let key = event.key;
  if (NOT_KEYS.includes(key)) return null;
  if (key === " ") key = "Space";
  else if (key.length === 1) {
    // Alt rewrites the character on some layouts: the key's own is what was meant.
    if (event.altKey && /^(Key|Digit)./.test(event.code)) key = event.code.slice(-1);
    key = key.toUpperCase();
  }
  // A character that is not a letter or a digit already says whether Shift is down.
  const shift = event.shiftKey && (key.length > 1 || /^[\p{L}\p{N}]$/u.test(key));
  return [event.ctrlKey || event.metaKey ? "Mod" : "", event.altKey ? "Alt" : "", shift ? "Shift" : "", key]
    .filter(Boolean)
    .join("+");
}

const SPELLING: Record<string, string> = {
  Mod: MAC ? "⌘" : "Ctrl",
  Shift: "⇧",
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Delete: "Del",
  Escape: "Esc",
  PageUp: "PgUp",
  PageDown: "PgDn",
};

/** A combination as a menu prints it: `Ctrl+⇧+S`. */
export function spell(combo: string): string {
  const [, mods, key] = /^((?:Mod\+|Alt\+|Shift\+)*)(.+)$/.exec(combo) ?? ["", "", combo];
  return [...mods.split("+").filter(Boolean), key].map((part) => SPELLING[part] ?? part).join("+");
}

const keymap = () => usePreferences.getState().saved.keymap ?? [];

const lookups = new WeakMap<ActionView[], Map<string, ActionView>>();

/** The action a key press is bound to, if any. */
export function actionOf(event: KeyboardEvent): ActionView | undefined {
  const list = keymap();
  let lookup = lookups.get(list);
  if (!lookup) {
    lookup = new Map(list.flatMap((action) => action.keys.map((key): [string, ActionView] => [key, action])));
    lookups.set(list, lookup);
  }
  const combo = comboOf(event);
  return combo ? lookup.get(combo) : undefined;
}

const hintIn = (list: ActionView[] | undefined, action: Action) => {
  const key = list?.find((entry) => entry.id === action)?.keys[0];
  return key ? spell(key) : undefined;
};

/** What a menu shows beside an action: its first key, spelled; nothing when it has none. */
export const keyHint = (action: Action): string | undefined => hintIn(keymap(), action);

/** The same for a component, which draws it again when the keys change. */
export const useKeyHint = (action: Action): string | undefined =>
  usePreferences((p) => hintIn(p.saved.keymap, action));
