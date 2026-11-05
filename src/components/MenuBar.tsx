import { useEffect, useRef } from "react";
import { useI18n } from "../i18n";
import { BrandMark } from "./BrandMark";
import { isDirty } from "../state/doc";
import { useDoc } from "../state/store";
import { closeMenu, openMenu, openSetup, useUi } from "../state/ui";
import { GearIcon } from "./icons";
import { BAR_MENUS, MOD_KEY } from "./menus";
import { WindowControls, hasNativeWindow } from "./WindowControls";

export function MenuBar() {
  const name = useDoc((doc) => doc.project.name);
  const dirty = useDoc(isDirty);
  // Only which title is open: the items themselves are built when it opens.
  const owner = useUi((ui) => ui.menu?.owner);
  const { t } = useI18n();
  const bar = useRef<HTMLDivElement>(null);

  const show = (index: number) => {
    const { id, items } = BAR_MENUS[index];
    const title = bar.current?.querySelector<HTMLElement>(`[data-menu="${id}"]`);
    if (!title) return;
    const { left, bottom } = title.getBoundingClientRect();
    openMenu({ x: left, y: bottom, owner: id, items: items(t) });
  };

  // With a menu open, Left and Right move along the bar.
  useEffect(() => {
    if (!owner) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      event.preventDefault();
      const at = BAR_MENUS.findIndex((menu) => menu.id === owner);
      show((at + (event.key === "ArrowLeft" ? -1 : 1) + BAR_MENUS.length) % BAR_MENUS.length);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  });

  return (
    <nav className="menubar" aria-label={t("app.title")} data-tauri-drag-region>
      <BrandMark />

      <div ref={bar} className="menu-group" role="menubar" aria-label={t("app.title")}>
        {BAR_MENUS.map(({ id, label }, index) => (
          <button
            key={id}
            type="button"
            role="menuitem"
            data-menu={id}
            aria-haspopup="menu"
            aria-expanded={owner === id}
            onClick={() => (owner === id ? closeMenu() : show(index))}
            // Once one is open, the others open by pointing at them, as in every desktop app.
            onPointerEnter={() => owner && owner !== id && show(index)}
          >
            {t(label)}
          </button>
        ))}
      </div>

      <p className="doc-name" title={name} data-tauri-drag-region>
        {name}
        {dirty ? (
          <span className="dirty" title={t("project.unsaved")}>
            <span aria-hidden="true">●</span>
            <span className="visually-hidden">{t("project.unsaved")}</span>
          </span>
        ) : null}
      </p>

      <div className="menu-end">
        <button
          type="button"
          className="icon-button"
          onClick={() => openSetup()}
          aria-label={t("menu.setup")}
          title={`${t("menu.setup")} (${MOD_KEY}+,)`}
        >
          <GearIcon />
        </button>
      </div>
      {hasNativeWindow() ? <WindowControls /> : null}
    </nav>
  );
}
