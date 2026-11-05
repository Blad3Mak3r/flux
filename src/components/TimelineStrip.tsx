import { memo, useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import { useShallow } from "zustand/react/shallow";
import { useT } from "../i18n";
import { activeTimeline } from "../state/doc";
import { useDoc } from "../state/store";
import { addTimeline, moveTimeline, openTimelineSettings, selectTimeline } from "../state/timelines";
import { menuAtPointer, openMenu } from "../state/ui";
import { dropIndex, dropLineY } from "../timeline/reorder";
import { selectMode, useTransport } from "../transport/store";
import { ChevronDownIcon, PlusIcon } from "./icons";
import { allTimelinesMenu, timelineMenu } from "./menus";

/** How far the pointer must go before a press on a tab is a drag and not a click. */
const DRAG_SLOP = 4;

/** One timeline. It re-renders only when its own name, or whether it is active, locked or dragged, changes. */
const Tab = memo(function Tab({
  id,
  name,
  active,
  playing,
  dragging,
  onGrab,
}: {
  id: string;
  name: string;
  active: boolean;
  /** The show is playing: this one is on air if it is active, and the others cannot be chosen. */
  playing: boolean;
  dragging: boolean;
  /** Starts dragging the tab to another place in the order. */
  onGrab: (event: PointerEvent<HTMLElement>, id: string) => void;
}) {
  const t = useT();
  const tab = useRef<HTMLButtonElement>(null);
  const label = name || t("timeline.default");
  const locked = playing && !active;

  // A timeline chosen from the list may be out of sight in the strip.
  useEffect(() => {
    if (active) tab.current?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [active]);

  return (
    <button
      ref={tab}
      type="button"
      role="tab"
      className="timeline-tab"
      aria-selected={active}
      aria-disabled={locked}
      tabIndex={active ? 0 : -1}
      data-playing={active && playing}
      data-dragging={dragging}
      onPointerDown={(event) => onGrab(event, id)}
      title={locked ? t("timeline.lockedPlaying") : label}
      onClick={() => void selectTimeline(id)}
      onDoubleClick={() => void openTimelineSettings(id)}
      onContextMenu={(event) => menuAtPointer(event, timelineMenu(t, id))}
    >
      <span className="timeline-name">{label}</span>
    </button>
  );
});

/** Moves the focus along the tabs; Enter chooses the one in focus. Choosing is not done by arrow
 * keys: it reopens the output, which a pass over several tabs must not do. */
function onTabsKeyDown(event: KeyboardEvent<HTMLDivElement>): void {
  const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
  if (!keys.includes(event.key)) return;
  const tabs = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]'));
  const at = tabs.indexOf(document.activeElement as HTMLElement);
  const next =
    event.key === "Home" ? 0
    : event.key === "End" ? tabs.length - 1
    : (at + (event.key === "ArrowLeft" ? -1 : 1) + tabs.length) % tabs.length;
  // The transport owns the arrow keys elsewhere: here they walk the strip.
  event.preventDefault();
  event.stopPropagation();
  tabs[next]?.focus();
}

/** The timelines of the project, in the bar of the Edit and Live views. The backend decides which is active. */
export function TimelineStrip() {
  const t = useT();
  const ids = useDoc(useShallow((doc) => doc.project.timelines.map((timeline) => timeline.id)));
  const names = useDoc(useShallow((doc) => doc.project.timelines.map((timeline) => timeline.name)));
  const active = useDoc((doc) => activeTimeline(doc).id);
  const playing = useTransport((transport) => selectMode(transport) === "playing");
  const list = useRef<HTMLDivElement>(null);
  // The tab being dragged, the index it would take if dropped now, and where the line that marks
  // it goes. The tab itself follows the pointer through its own style, so a move does not render.
  const [drag, setDrag] = useState<{ id: string; line: number | null } | null>(null);

  const grab = useCallback((event: PointerEvent<HTMLElement>, id: string) => {
    const tab = event.currentTarget;
    const tabs = Array.from(list.current?.querySelectorAll<HTMLElement>('[role="tab"]') ?? []);
    const from = tabs.indexOf(tab);
    if (event.button !== 0 || from === -1) return;
    const widths = tabs.map((node) => node.offsetWidth);
    const before = widths.slice(0, from).reduce((sum, width) => sum + width, 0);
    const after = widths.slice(from + 1).reduce((sum, width) => sum + width, 0);
    const startX = event.clientX;
    let to = from;
    let moved = false;

    const onMove = (move: globalThis.PointerEvent) => {
      const dx = Math.min(Math.max(move.clientX - startX, -before), after);
      if (!moved) {
        if (Math.abs(dx) < DRAG_SLOP) return;
        moved = true;
        setDrag({ id, line: null });
      }
      tab.style.transform = `translateX(${dx}px)`;
      const next = dropIndex(widths, from, dx);
      if (next !== to) {
        to = next;
        setDrag({ id, line: to === from ? null : dropLineY(widths, from, to) });
      }
    };
    const onKey = (key: globalThis.KeyboardEvent) => key.key === "Escape" && finish(false);
    const finish = (drop: boolean) => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("keydown", onKey);
      tab.style.transform = "";
      setDrag(null);
      if (!moved) return;
      // The click that follows a drag must not choose the tab.
      const swallow = (click: Event) => click.stopPropagation();
      window.addEventListener("click", swallow, { capture: true, once: true });
      setTimeout(() => window.removeEventListener("click", swallow, { capture: true }), 0);
      if (drop && to !== from) void moveTimeline(id, to);
    };
    const onUp = () => finish(true);
    const onCancel = () => finish(false);
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("keydown", onKey);
  }, []);

  return (
    <div className="timeline-strip">
      <div
        ref={list}
        className="timeline-tabs"
        role="tablist"
        aria-label={t("timeline.strip")}
        onKeyDown={onTabsKeyDown}
        // A wheel turned over the strip walks it sideways.
        onWheel={(event) => {
          event.currentTarget.scrollLeft += event.deltaY;
        }}
      >
        {ids.map((id, index) => (
          <Tab
            key={id}
            id={id}
            name={names[index]}
            active={id === active}
            playing={playing}
            dragging={drag?.id === id}
            onGrab={grab}
          />
        ))}
        {drag?.line != null && <div className="timeline-drop" style={{ left: drag.line }} />}
      </div>
      <button
        type="button"
        className="icon-button"
        aria-label={t("timeline.add")}
        title={t("timeline.add")}
        onClick={() => void addTimeline(t)}
      >
        <PlusIcon />
      </button>
      <button
        type="button"
        className="icon-button"
        aria-label={t("timeline.all")}
        title={t("timeline.all")}
        aria-haspopup="menu"
        onClick={(event) => {
          const box = event.currentTarget.getBoundingClientRect();
          openMenu({ x: box.left, y: box.bottom, items: allTimelinesMenu(t) });
        }}
      >
        <ChevronDownIcon />
      </button>
    </div>
  );
}
