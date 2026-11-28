import type { MouseEvent } from "react";
import { useT } from "../i18n";
import { cycleRipple, openMenu, useUi } from "../state/ui";
import { ChevronDownIcon } from "./icons";
import { rippleMenu } from "./menus";

/**
 * Ripple editing: a lamp and the mode's name that step through off, per track and all tracks, and
 * beside them the menu to pick one. Styled as the Snap control it sits next to.
 */
export function RippleControl() {
  const t = useT();
  const mode = useUi((ui) => ui.ripple);
  const options = (event: MouseEvent<HTMLButtonElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    openMenu({ x: box.left, y: box.bottom, items: rippleMenu(t) });
  };
  return (
    <div className="snap-control" role="group" aria-label={t("ripple.label")}>
      <button type="button" className="snap-toggle" aria-pressed={mode !== "off"} title={t("ripple.hint")} onClick={cycleRipple}>
        <span className="lamp" data-state={mode === "off" ? "off" : "on"} aria-hidden="true" />
        {t("ripple.label")}
        {mode !== "off" && <span className="ripple-mode">{t(`ripple.mode.${mode}`)}</span>}
      </button>
      <button type="button" className="snap-options" aria-haspopup="menu" title={t("ripple.options")} onClick={options}>
        <ChevronDownIcon />
        <span className="visually-hidden">{t("ripple.options")}</span>
      </button>
    </div>
  );
}
