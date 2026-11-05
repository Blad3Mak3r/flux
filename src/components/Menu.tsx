import { useEffect, useLayoutEffect, useRef } from "react";
import { closeMenu, useUi } from "../state/ui";
import type { MenuItem, OpenMenu } from "../state/ui";
import { CheckIcon, ChevronRightIcon } from "./icons";

/** Room kept between a menu and the window edge. */
const EDGE = 4;

const ITEMS = '[role^="menuitem"]:not(:disabled)';

/** Room a submenu needs beside its parent; when the window lacks it, it opens to the left. */
const SUB_WIDTH = 320;

/** The items of one menu level, leaving out those of the submenus nested inside it. */
const levelItems = (level: Element) =>
  [...level.querySelectorAll<HTMLElement>(ITEMS)].filter((el) => el.closest('[role="menu"]') === level);

function Items({ items }: { items: MenuItem[] }) {
  return items.map((item, index) => {
    if (item === "-") return <div key={index} className="menu-rule" role="separator" />;
    if ("submenu" in item) {
      return (
        // Open while pointed at or, from the keyboard, while `data-open` is set; leaving by focus closes it.
        <div
          key={index}
          className="menu-host"
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget)) delete event.currentTarget.dataset.open;
          }}
        >
          <button
            type="button"
            className="menu-item"
            role="menuitem"
            aria-haspopup="menu"
            disabled={item.disabled}
            onPointerEnter={(event) => event.currentTarget.focus()}
          >
            <span className="menu-check" />
            <span className="menu-label">{item.label}</span>
            <span className="menu-arrow">
              <ChevronRightIcon />
            </span>
          </button>
          <div className="menu menu-sub" role="menu">
            <Items items={item.submenu} />
          </div>
        </div>
      );
    }
    return (
      <button
        key={index}
        type="button"
        className="menu-item"
        role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
        aria-checked={item.checked}
        disabled={item.disabled}
        onPointerEnter={(event) => event.currentTarget.focus()}
        onClick={() => {
          closeMenu();
          item.run();
        }}
      >
        <span className="menu-check">{item.checked ? <CheckIcon /> : null}</span>
        <span className="menu-label">{item.label}</span>
        {item.shortcut ? <kbd className="menu-keys">{item.shortcut}</kbd> : null}
      </button>
    );
  });
}

function MenuPopup({ menu }: { menu: OpenMenu }) {
  const box = useRef<HTMLDivElement>(null);

  // Placed before the first paint: where the pointer is, flipped up or left when it would run out of the window.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    el.style.left = `${Math.max(EDGE, Math.min(menu.x, window.innerWidth - width - EDGE))}px`;
    el.style.top = `${Math.max(EDGE, menu.y + height > window.innerHeight - EDGE ? menu.y - height : menu.y)}px`;
    el.classList.toggle("menu-flip", el.getBoundingClientRect().right + SUB_WIDTH > window.innerWidth);
    el.querySelector<HTMLElement>(ITEMS)?.focus();
  }, [menu]);

  // Listens only while a menu is open. The keyboard belongs to the menu: the transport and
  // editing shortcuts must not fire behind it, so keys are taken before the shortcut handler sees them.
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target instanceof Element ? event.target : null;
      // The menu bar title that owns the menu toggles it itself on click.
      if (!target || el.contains(target) || target.closest('[aria-haspopup="menu"]')) return;
      closeMenu();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Tab") return closeMenu();
      const active = document.activeElement as HTMLElement | null;
      const host = active?.closest<HTMLElement>(".menu-host") ?? null;
      const parent = active?.closest<HTMLElement>(".menu-sub")?.parentElement ?? null; // the host of the open submenu
      const opens = active?.getAttribute("aria-haspopup") === "menu" && active.closest(".menu") !== null;
      const into = (key: string) => key === "ArrowRight" || key === "Enter" || key === " ";
      if (opens && into(event.key) && host) {
        // Open the submenu from the keyboard and step into it.
        event.preventDefault();
        event.stopPropagation();
        host.dataset.open = "";
        levelItems(host.querySelector(".menu-sub")!)[0]?.focus();
        return;
      }
      if (event.key === "ArrowLeft" && parent) {
        // Back to the row that opened the submenu; the menu bar does not move.
        event.preventDefault();
        event.stopPropagation();
        delete parent.dataset.open;
        parent.querySelector<HTMLElement>(":scope > .menu-item")?.focus();
        return;
      }
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") return event.stopPropagation(); // the menu bar moves along
      event.stopPropagation();
      if (event.key === "Escape") {
        event.preventDefault();
        closeMenu();
        return;
      }
      const items = levelItems(active?.closest('[role="menu"]') ?? el);
      const at = items.indexOf(document.activeElement as HTMLElement);
      const step =
        event.key === "ArrowDown" ? items[(at + 1) % items.length]
        : event.key === "ArrowUp" ? items[(at - 1 + items.length) % items.length]
        : event.key === "Home" ? items[0]
        : event.key === "End" ? items[items.length - 1]
        : null;
      if (step) {
        event.preventDefault();
        step.focus();
      } else if (event.key === "Enter" || event.key === " ") {
        // A key-up handler in the shell cancels the click of Space, so activate here for both.
        event.preventDefault();
        (document.activeElement as HTMLElement | null)?.click();
      }
    };
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("blur", closeMenu);
    window.addEventListener("resize", closeMenu);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("blur", closeMenu);
      window.removeEventListener("resize", closeMenu);
    };
  }, []);

  return (
    <div ref={box} className="menu" role="menu" style={{ left: menu.x, top: menu.y }}>
      <Items items={menu.items} />
    </div>
  );
}

/** The one menu on show, whichever opened it. */
export function MenuHost() {
  const menu = useUi((ui) => ui.menu);
  return menu ? <MenuPopup menu={menu} /> : null;
}
