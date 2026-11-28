import { useT } from "../i18n";
import { setView, useUi } from "../state/ui";
import type { View } from "../state/ui";
import { MOD_KEY } from "../components/menus";
import { selectMode, useTransport } from "../transport/store";
import { useShowLock } from "./lockStore";

const VIEWS: { id: View; key: "1" | "2" }[] = [
  { id: "edit", key: "1" },
  { id: "live", key: "2" },
];

/** Chooses between editing the timeline and running it; it sits in the middle of the menu bar. */
export function ViewSwitch() {
  const t = useT();
  const view = useUi((ui) => ui.view);
  const playing = useTransport(selectMode) === "playing";
  const showLocked = useShowLock((lock) => lock.locked);
  return (
    <div className="view-switch" role="group" aria-label={t("view.label")}>
      {VIEWS.map(({ id, key }) => (
        <button
          key={id}
          type="button"
          aria-pressed={view === id}
          title={
            showLocked && view !== id
              ? t("live.lock.blocked")
              : playing && view !== id
                ? t("view.lockedPlaying")
                : `${t(`view.${id}`)} (${MOD_KEY}+${key})`
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
