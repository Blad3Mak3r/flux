import type { MouseEvent } from "react";
import { useT } from "../i18n";
import { openMenu, toggleSnap, useUi } from "../state/ui";
import { ChevronDownIcon } from "./icons";
import { snapMenu } from "./menus";

/**
 * The magnet: a lamp and a name that turn it on and off, and beside them the menu of what it pulls
 * onto and how fine its grid is.
 */
export function SnapControl() {
  const t = useT();
  const on = useUi((ui) => ui.snap);
  const options = (event: MouseEvent<HTMLButtonElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    openMenu({ x: box.left, y: box.bottom, items: snapMenu(t) });
  };
  return (
    <div className="snap-control" role="group" aria-label={t("snap.label")}>
      <button type="button" className="snap-toggle" aria-pressed={on} title={t("snap.hint")} onClick={toggleSnap}>
        <span className="lamp" data-state={on ? "on" : "off"} aria-hidden="true" />
        {t("snap.label")}
      </button>
      <button type="button" className="snap-options" aria-haspopup="menu" title={t("snap.options")} onClick={options}>
        <ChevronDownIcon />
        <span className="visually-hidden">{t("snap.options")}</span>
      </button>
    </div>
  );
}
