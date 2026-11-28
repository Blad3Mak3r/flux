import { useState } from "react";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { Action } from "../keymap/actions";
import { comboOf, spell } from "../keymap/keys";
import type { ActionView } from "../keymap/keys";
import { changePreferences, usePreferences } from "../preferences/store";
import { errorMessage } from "../project/api";

const EMPTY: ActionView[] = [];
const same = (a: string[], b: string[]) => a.length === b.length && a.every((key, at) => key === b[at]);

/**
 * The keys of a cue show. The backend holds the table, refuses a key another action has and keeps
 * the changes on this computer; this page shows them and sends what the user pressed.
 */
export function ShortcutsPage() {
  const t = useT();
  const keymap = usePreferences((p) => p.saved.keymap) ?? EMPTY;
  const [recording, setRecording] = useState<Action | null>(null);
  const [problem, setProblem] = useState<{ action: Action; text: string } | null>(null);
  const groups = [...new Set(keymap.map((entry) => entry.group))];
  const changed = keymap.some((entry) => !same(entry.keys, entry.defaults));

  const send = (action: Action | null, change: object, combo?: string) =>
    changePreferences(change, false).then(
      () => setProblem(null),
      (err) => {
        const message = errorMessage(err);
        const holder = /^conflict:(\w+)$/.exec(message)?.[1];
        const text = holder
          ? t("shortcuts.conflict", { keys: spell(combo ?? ""), action: t(`shortcuts.action.${holder}` as MessageKey) })
          : message;
        if (action) setProblem({ action, text });
      },
    );

  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("shortcuts.intro")}</p>
      <div className="button-row">
        <button type="button" className="button" disabled={!changed} onClick={() => void send(null, { op: "shortcutReset", action: null })}>
          {t("shortcuts.resetAll")}
        </button>
      </div>
      {groups.map((group) => (
        <section key={group} className="shortcut-group" aria-labelledby={`shortcuts-${group}`}>
          <h4 id={`shortcuts-${group}`}>{t(`shortcuts.group.${group}` as MessageKey)}</h4>
          <ul>
            {keymap
              .filter((entry) => entry.group === group)
              .map((entry) => {
                const name = t(`shortcuts.action.${entry.id}` as MessageKey);
                const isRecording = recording === entry.id;
                const failed = problem?.action === entry.id ? problem.text : null;
                return (
                  <li key={entry.id} className="shortcut-row">
                    <span className="shortcut-name">{name}</span>
                    <button
                      type="button"
                      className="shortcut-keys"
                      data-recording={isRecording || undefined}
                      aria-label={`${name}: ${isRecording ? t("shortcuts.recording") : entry.keys.map(spell).join(", ") || t("shortcuts.none")}`}
                      onClick={() => {
                        setProblem(null);
                        setRecording(entry.id);
                      }}
                      onBlur={() => setRecording(null)}
                      onKeyDown={(event) => {
                        if (!isRecording || event.key === "Tab") return;
                        event.preventDefault();
                        event.stopPropagation();
                        if (event.key === "Escape") return setRecording(null);
                        const combo = comboOf(event.nativeEvent);
                        if (!combo) return;
                        setRecording(null);
                        void send(entry.id, { op: "shortcut", action: entry.id, key: combo }, combo);
                      }}
                    >
                      {isRecording ? (
                        <span className="shortcut-hint">{t("shortcuts.recording")}</span>
                      ) : entry.keys.length === 0 ? (
                        <span className="shortcut-hint">{t("shortcuts.none")}</span>
                      ) : (
                        entry.keys.map((key) => <kbd key={key}>{spell(key)}</kbd>)
                      )}
                    </button>
                    <button
                      type="button"
                      className="button"
                      disabled={entry.keys.length === 0}
                      onClick={() => void send(entry.id, { op: "shortcut", action: entry.id, key: null })}
                    >
                      {t("shortcuts.clear")}
                    </button>
                    <button
                      type="button"
                      className="button"
                      disabled={same(entry.keys, entry.defaults)}
                      onClick={() => void send(entry.id, { op: "shortcutReset", action: entry.id })}
                    >
                      {t("shortcuts.reset")}
                    </button>
                    {failed && (
                      <p className="error shortcut-error" role="alert">
                        {failed}
                      </p>
                    )}
                  </li>
                );
              })}
          </ul>
        </section>
      ))}
    </div>
  );
}
