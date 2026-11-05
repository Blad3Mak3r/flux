import { useT } from "../i18n";
import { setView, useUi } from "../state/ui";
import type { View } from "../state/ui";
import { MOD_KEY } from "../components/menus";

const VIEWS: { id: View; key: "1" | "2" }[] = [
  { id: "edit", key: "1" },
  { id: "live", key: "2" },
];

/** Chooses between editing the timeline and running it; each view carries it at the end of its bar. */
export function ViewSwitch() {
  const t = useT();
  const view = useUi((ui) => ui.view);
  return (
    <div className="view-switch" role="group" aria-label={t("view.label")}>
      {VIEWS.map(({ id, key }) => (
        <button
          key={id}
          type="button"
          aria-pressed={view === id}
          title={`${t(`view.${id}`)} (${MOD_KEY}+${key})`}
          onClick={() => setView(id)}
        >
          {t(`view.${id}`)}
        </button>
      ))}
    </div>
  );
}
