import { memo, useEffect, useRef } from "react";
import type { KeyboardEvent } from "react";
import { useShallow } from "zustand/react/shallow";
import { useT } from "../i18n";
import { activeTimeline } from "../state/doc";
import { useDoc } from "../state/store";
import { addTimeline, openTimelineSettings, selectTimeline } from "../state/timelines";
import { menuAtPointer, openMenu } from "../state/ui";
import { selectMode, useTransport } from "../transport/store";
import { ChevronDownIcon, PlusIcon } from "./icons";
import { allTimelinesMenu, timelineMenu } from "./menus";

/** One timeline. It re-renders only when its own name, or whether it is active or locked, changes. */
const Tab = memo(function Tab({
  id,
  name,
  active,
  playing,
}: {
  id: string;
  name: string;
  active: boolean;
  /** The show is playing: this one is on air if it is active, and the others cannot be chosen. */
  playing: boolean;
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
  return (
    <div className="timeline-strip">
      <div
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
          <Tab key={id} id={id} name={names[index]} active={id === active} playing={playing} />
        ))}
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
