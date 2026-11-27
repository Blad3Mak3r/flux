import { Children, Fragment, isValidElement, useId } from "react";
import type { MouseEvent, ReactNode } from "react";
import { closeMenu, openMenu, useUi } from "../state/ui";
import type { MenuItem } from "../state/ui";
import { ChevronDownIcon } from "./icons";

const text = (node: ReactNode): string =>
  Array.isArray(node)
    ? node.map(text).join("")
    : typeof node === "string" || typeof node === "number"
      ? String(node)
      : isValidElement<{ children?: ReactNode }>(node)
        ? text(node.props.children)
        : "";

/** Reads `<option>` and `<optgroup>` children (through fragments and arrays) into the rows of a menu. */
function rows(children: ReactNode, value: string, pick: (value: string) => void): { items: MenuItem[]; label: string } {
  const items: MenuItem[] = [];
  let label = "";
  const walk = (nodes: ReactNode) =>
    Children.toArray(nodes).forEach((node) => {
      if (!isValidElement<{ children?: ReactNode; value?: string | number; disabled?: boolean; label?: string }>(node)) return;
      const { props } = node;
      if (node.type === Fragment) return walk(props.children);
      if (node.type === "optgroup") {
        items.push({ heading: props.label ?? "" });
        return walk(props.children);
      }
      if (node.type !== "option") return;
      const v = String(props.value ?? text(props.children));
      const name = text(props.children);
      if (v === value) label = name;
      items.push({ label: name, checked: v === value, disabled: props.disabled, run: () => pick(v) });
    });
  walk(children);
  return { items, label };
}

/**
 * A drop-down that draws its list with the app's own menu, since the list of a native `<select>`
 * is drawn by the OS and ignores the theme. The options are written as `<option>` / `<optgroup>` children.
 */
export function Select({
  value,
  onChange,
  children,
  id,
  className,
  disabled,
  title,
  autoFocus,
  onClick,
  ...aria
}: {
  value: string | number;
  onChange: (value: string) => void;
  children: ReactNode;
  id?: string;
  className?: string;
  disabled?: boolean;
  title?: string;
  autoFocus?: boolean;
  onClick?: (event: MouseEvent) => void;
  "aria-label"?: string;
  "aria-describedby"?: string;
}) {
  const own = useId();
  const key = id ?? own;
  const open = useUi((ui) => ui.menu?.select?.id === key);
  const { items, label } = rows(children, String(value), onChange);
  return (
    <button
      type="button"
      id={id}
      className={className ? `select ${className}` : "select"}
      disabled={disabled}
      title={title}
      autoFocus={autoFocus}
      aria-haspopup="listbox"
      aria-expanded={open}
      {...aria}
      onClick={(event) => {
        onClick?.(event);
        if (open) return closeMenu();
        const box = event.currentTarget.getBoundingClientRect();
        openMenu({ x: box.left, y: box.bottom, items, select: { id: key, width: box.width } });
      }}
      onKeyDown={(event) => {
        // Enter and Space click a button already; ArrowDown opens it like a native select.
        if (event.key === "ArrowDown" && !open) {
          event.preventDefault();
          event.currentTarget.click();
        }
      }}
    >
      <span className="select-value">{label}</span>
      <span className="select-chevron" aria-hidden="true">
        <ChevronDownIcon />
      </span>
    </button>
  );
}
