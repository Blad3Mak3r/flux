// The VJ show's own shortcuts: only the ones Resolume has by default, so every other key is free to map. The list is
// the backend's file (`vj_shortcuts.json`): it refuses to learn exactly these keys, and the keys and the menus here read
// the same entries. Nothing else is bound; plain and Shift keys belong to the user's keyboard mapping.

import list from "../../src-tauri/crates/flux-project/src/vj_shortcuts.json";
import { MOD_KEY } from "../components/menus";

export type ShortcutAction = (typeof list)[number]["action"];

/** What the key pressed does in the app, if it is one of the shortcuts. */
export function shortcutOf(event: KeyboardEvent): ShortcutAction | undefined {
  if (event.altKey) return undefined;
  const cmd = event.ctrlKey || event.metaKey;
  return list.find((s) => s.code === event.code && s.cmd === cmd && s.shift === event.shiftKey)?.action;
}

/** The accelerator a menu shows for an action, as `Ctrl+⇧+S`. */
export function acceleratorOf(action: ShortcutAction): string | undefined {
  const s = list.find((entry) => entry.action === action);
  if (!s) return undefined;
  const key = s.code === "Escape" ? "Esc" : s.code.replace(/^(Key|Digit)/, "");
  return `${s.cmd ? `${MOD_KEY}+` : ""}${s.shift ? "⇧+" : ""}${key}`;
}
