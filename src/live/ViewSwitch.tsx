import { useT } from "../i18n";
import { setView, useUi } from "../state/ui";
import type { View } from "../state/ui";
import { useKeyHint } from "../keymap/keys";
import { selectMode, useTransport } from "../transport/store";
import { useShowLock } from "./lockStore";

const VIEWS: View[] = ["edit", "live"];

/** Chooses between editing the timeline and running it; it sits in the middle of the menu bar. */
export function ViewSwitch() {
  const t = useT();
  const view = useUi((ui) => ui.view);
  const playing = useTransport(selectMode) === "playing";
  const showLocked = useShowLock((lock) => lock.locked);
  const hints = { edit: useKeyHint("viewEdit"), live: useKeyHint("viewLive") };
  return (
    <div className="view-switch" role="group" aria-label={t("view.label")}>
      {VIEWS.map((id) => (
        <button
          key={id}
          type="button"
          aria-pressed={view === id}
          title={
            showLocked && view !== id
              ? t("live.lock.blocked")
              : playing && view !== id
                ? t("view.lockedPlaying")
                : hints[id] ? `${t(`view.${id}`)} (${hints[id]})` : t(`view.${id}`)
          }
          disabled={(playing || showLocked) && view !== id}
          onClick={() => setView(id)}
        >
          {t(`view.${id}`)}
        </button>
      ))}
    </div>
  );
}
