// Drags inside the window, made with the pointer. The webview's own drag and drop is left to the
// files that come from the file explorer (see fileDrop.ts): once it is on, it no longer runs
// inside the page on Windows, so nothing here may rely on `draggable` or `dataTransfer`.

import { useCallback, useRef } from "react";

/** What a drag carries: values by kind, so a target can tell whether it takes the drag. */
export type DragData = Record<string, string>;

/** Where the pointer is, in client pixels. */
export interface DragAt {
  x: number;
  y: number;
}

/** What takes a drag dropped on it. `over` hears the pointer enter, move over and leave it. */
export interface DropTarget {
  accepts: (data: DragData) => boolean;
  over?: (on: boolean, at: DragAt) => void;
  drop: (data: DragData, at: DragAt) => void;
}

/** Pixels the pointer must move before a press becomes a drag. */
const THRESHOLD = 4;

const targets = new Map<Element, DropTarget>();

// A native drag of an image or a link would take the pointer from us.
if (typeof document !== "undefined") document.addEventListener("dragstart", (event) => event.preventDefault());

/** Makes the element it is given the home of `target`: the drop target of drags it accepts. */
export function useDropTarget(target: DropTarget) {
  const latest = useRef(target);
  latest.current = target;
  const element = useRef<Element | null>(null);
  const proxy = useRef<DropTarget>({
    accepts: (data) => latest.current.accepts(data),
    over: (on, at) => latest.current.over?.(on, at),
    drop: (data, at) => latest.current.drop(data, at),
  });
  return useCallback((el: Element | null) => {
    if (element.current) targets.delete(element.current);
    element.current = el;
    if (el) targets.set(el, proxy.current);
  }, []);
}

function targetAt(at: DragAt, data: DragData): DropTarget | null {
  for (let el = document.elementFromPoint(at.x, at.y); el; el = el.parentElement) {
    const target = targets.get(el);
    if (target?.accepts(data)) return target;
  }
  return null;
}

/**
 * Starts a drag of `data` from a press: it begins once the pointer has moved a few pixels, shows
 * `label` at the pointer, and drops on the target under it when the button is let go. Escape
 * cancels it. A press that never moves is left to be a click.
 */
export function startDrag(event: { button: number; clientX: number; clientY: number }, data: DragData, label = ""): void {
  if (event.button !== 0) return;
  const from: DragAt = { x: event.clientX, y: event.clientY };
  let ghost: HTMLElement | null = null;
  let over: DropTarget | null = null;

  const move = (e: PointerEvent) => {
    const at = { x: e.clientX, y: e.clientY };
    if (!ghost) {
      if (Math.hypot(at.x - from.x, at.y - from.y) < THRESHOLD) return;
      ghost = document.createElement("div");
      ghost.className = "drag-ghost";
      ghost.textContent = label;
      ghost.hidden = label === "";
      document.body.append(ghost);
      document.body.classList.add("dragging");
    }
    ghost.style.transform = `translate(${at.x + 12}px, ${at.y + 12}px)`;
    window.getSelection()?.removeAllRanges();
    const next = targetAt(at, data);
    if (next !== over) over?.over?.(false, at);
    over = next;
    over?.over?.(true, at);
  };
  const end = (drop: boolean, e?: PointerEvent) => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
    window.removeEventListener("keydown", key, true);
    if (!ghost) return;
    ghost.remove();
    document.body.classList.remove("dragging");
    const at = e ? { x: e.clientX, y: e.clientY } : from;
    over?.over?.(false, at);
    if (drop) over?.drop(data, at);
    // The click that follows the release belongs to the drag, not to what is under the pointer.
    const swallow = (click: Event) => {
      click.stopPropagation();
      click.preventDefault();
    };
    window.addEventListener("click", swallow, true);
    setTimeout(() => window.removeEventListener("click", swallow, true), 50);
  };
  const up = (e: PointerEvent) => end(true, e);
  const cancel = () => end(false);
  const key = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.stopPropagation();
    end(false);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", cancel);
  window.addEventListener("keydown", key, true);
}
