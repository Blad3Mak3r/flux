// The dockable panels of the VJ screen, drawn from the layout tree the backend keeps (`vj_layout.rs`): splits with
// their splitters, tab groups with their strips, and the deck, which is a leaf of its own. The backend owns every
// rule (what may go where, the minimum sizes, what is normalised); this draws the tree, follows the pointer for a
// drag or a splitter, and forwards the end of it.

import { Fragment, createContext, useContext, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent, ReactNode, RefObject, WheelEvent } from "react";
import { CloseIcon } from "../components/icons";
import { useT } from "../i18n";
import type { Translate } from "../i18n";
import { startDrag, useDropTarget } from "../state/pointerDrag";
import type { DragAt } from "../state/pointerDrag";
import type { DockNode, DockZone, LayoutState, MonitorKind } from "./api";
import { layoutActivate, layoutClose, layoutMove, layoutReorder, layoutResize, useVj } from "./store";

/** What a dragged tab carries: the id of its panel. */
const PANEL = "application/x-flux-vj-panel";
/** The share a keyboard step moves a splitter by. */
const STEP = 0.02;
/** How close to an edge, as a share of the target, a drop takes that edge. */
const EDGE = 0.25;

const nodeId = (node: DockNode) => (node.type === "deck" ? "deck" : node.id);

const KIND_TITLES: Record<MonitorKind["type"], "vj.composition" | "vj.preview" | "vj.inspector.layer" | "vj.inspector.group" | "vj.monitor.screen"> = {
  composition: "vj.composition",
  preview: "vj.preview",
  layer: "vj.inspector.layer",
  group: "vj.inspector.group",
  screen: "vj.monitor.screen",
};

/** The name of a monitor's kind. */
export const kindTitle = (t: Translate, kind: MonitorKind) => t(KIND_TITLES[kind.type]);

const FIXED_TITLES = {
  composition: "vj.inspector.composition",
  group: "vj.inspector.group",
  layer: "vj.inspector.layer",
  clip: "vj.inspector.clip",
  mappings: "vj.inspector.mappings",
  dashboard: "vj.dashboard.title",
  files: "vj.media",
  generators: "vj.browser.generators",
  effects: "vj.effects",
} as const;

/** The fixed panels, in the order the View menu lists them. */
export const FIXED_PANELS = ["clip", "layer", "group", "composition", "dashboard", "mappings", "files", "generators", "effects"] as const;

/** The title of a panel: a fixed one by its name, a monitor by what it shows. */
export function panelTitle(t: Translate, id: string, layout: LayoutState): string {
  if (id in FIXED_TITLES) return t(FIXED_TITLES[id as keyof typeof FIXED_TITLES]);
  const monitor = layout.current.monitors.find((m) => m.id === id);
  return monitor ? t("vj.monitor.title", { kind: kindTitle(t, monitor.kind) }) : id;
}

interface DockContext {
  state: LayoutState;
  deck: ReactNode;
  renderPanel: (id: string) => ReactNode;
}
const Dock$ = createContext<DockContext | null>(null);
const useDock = () => {
  const dock = useContext(Dock$);
  if (!dock) throw new Error("outside the dock");
  return dock;
};

// ---- Dropping a tab ----

/** Where a dragged tab would land: the zone of the target it lights, and for a strip the mark where it goes. */
interface Hover {
  zone: DockZone | null;
  /** The tab strip: where the tab would be put, as a position in the target. */
  mark?: { x: number; y: number; h: number };
  reorder?: { from: number; to: number };
}

/** The edge of `rect` nearest to the pointer, or the centre when it is not within {@link EDGE} of one and the target takes tabs. */
function zoneAt(rect: DOMRect, at: DragAt, centre: boolean): DockZone {
  const edges: [DockZone, number][] = [
    ["left", (at.x - rect.left) / rect.width],
    ["right", (rect.right - at.x) / rect.width],
    ["top", (at.y - rect.top) / rect.height],
    ["bottom", (rect.bottom - at.y) / rect.height],
  ];
  const [zone, near] = edges.reduce((best, edge) => (edge[1] < best[1] ? edge : best));
  return centre && near >= EDGE ? "center" : zone;
}

/**
 * Makes an element the drop target of tabs, for the tab group `node` (with its `strip` of tabs) or the deck (no strip,
 * no centre). Dropping on the strip of the group the tab is in reorders it; anywhere else moves it.
 */
function useTabDrop(node: string, strip: RefObject<HTMLElement | null> | null, panels: string[]) {
  const [hover, setHover] = useState<Hover | null>(null);
  const element = useRef<HTMLElement | null>(null);
  const dragged = useRef("");
  const aim = (at: DragAt): Hover | null => {
    const box = element.current?.getBoundingClientRect();
    if (!box) return null;
    const bar = strip?.current?.getBoundingClientRect();
    if (strip?.current && bar && at.y >= bar.top && at.y <= bar.bottom) {
      const from = panels.indexOf(dragged.current);
      const others = [...strip.current.querySelectorAll<HTMLElement>("[data-tab]")].filter((tab) => tab.dataset.tab !== dragged.current);
      const to = others.filter((tab) => {
        const r = tab.getBoundingClientRect();
        return at.x > r.left + r.width / 2;
      }).length;
      const x = (to < others.length ? others[to].getBoundingClientRect().left : (others[others.length - 1]?.getBoundingClientRect().right ?? bar.left)) - box.left;
      return { zone: from < 0 ? "center" : null, mark: { x, y: bar.top - box.top, h: bar.height }, reorder: from < 0 ? undefined : { from, to } };
    }
    return { zone: zoneAt(box, at, strip !== null) };
  };
  const ref = useDropTarget({
    accepts: (data) => {
      dragged.current = data[PANEL] ?? "";
      return PANEL in data;
    },
    over: (on, at) => setHover(on ? aim(at) : null),
    drop: (data, at) => {
      const there = aim(at);
      if (there?.reorder) {
        if (there.reorder.from !== there.reorder.to) void layoutReorder(node, there.reorder.from, there.reorder.to);
      } else if (there?.zone) void layoutMove(data[PANEL], { node, zone: there.zone });
    },
  });
  return {
    hover,
    ref: (el: HTMLElement | null) => {
      element.current = el;
      ref(el);
    },
  };
}

function DropOverlay({ hover }: { hover: Hover | null }) {
  if (!hover) return null;
  return (
    <>
      {hover.zone && <div className={`vj-drop ${hover.zone}`} aria-hidden="true" />}
      {hover.mark && <div className="vj-drop-mark" style={{ left: hover.mark.x, top: hover.mark.y, height: hover.mark.h }} aria-hidden="true" />}
    </>
  );
}

// ---- The tree ----

function Tab({ id, active, group, index, title }: { id: string; active: boolean; group: string; index: number; title: string }) {
  const t = useT();
  const activate = () => {
    if (!active) void layoutActivate(group, index);
  };
  return (
    <div
      role="tab"
      tabIndex={0}
      aria-selected={active}
      className={`vj-dtab${active ? " on" : ""}`}
      data-tab={id}
      title={title}
      onPointerDown={(event) => startDrag(event, { [PANEL]: id }, title)}
      onClick={activate}
      onKeyDown={(event: KeyboardEvent) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          activate();
        }
      }}
    >
      <span className="vj-dtab-t">{title}</span>
      <button type="button" className="vj-dtab-x" title={t("vj.dock.close")} aria-label={`${t("vj.dock.close")}: ${title}`}
        onPointerDown={(event) => event.stopPropagation()}
        onClick={(event) => { event.stopPropagation(); void layoutClose(id); }}>
        <CloseIcon />
      </button>
    </div>
  );
}

function Group({ node }: { node: Extract<DockNode, { type: "tabs" }> }) {
  const t = useT();
  const { state, renderPanel } = useDock();
  const strip = useRef<HTMLDivElement>(null);
  const drop = useTabDrop(node.id, strip, node.panels);
  const active = node.panels[node.active];
  // A strip is a row of tabs that scrolls sideways: the wheel moves it.
  const wheel = (event: WheelEvent) => {
    if (strip.current && event.deltaY !== 0) strip.current.scrollLeft += event.deltaY;
  };
  return (
    <section className="vj-panel vj-group" ref={drop.ref}>
      <div className="vj-dtabs" role="tablist" aria-label={t("vj.dock.tabs")} ref={strip} onWheel={wheel}>
        {node.panels.map((id, index) => (
          <Tab key={id} id={id} group={node.id} index={index} active={id === active} title={panelTitle(t, id, state)} />
        ))}
      </div>
      <div className="vj-dbody" role="tabpanel">{renderPanel(active)}</div>
      <DropOverlay hover={drop.hover} />
    </section>
  );
}

function DeckSlot() {
  const { deck } = useDock();
  const drop = useTabDrop("deck", null, []);
  return (
    <div className="vj-deck-slot" ref={drop.ref}>
      {deck}
      <DropOverlay hover={drop.hover} />
    </div>
  );
}

function Split({ node }: { node: Extract<DockNode, { type: "split" }> }) {
  const t = useT();
  const { state } = useDock();
  const box = useRef<HTMLDivElement>(null);
  // The shares while a splitter is held, until the backend's answer replaces them.
  const [live, setLive] = useState<number[] | null>(null);
  const sizes = live ?? node.sizes;
  const row = node.dir === "row";
  const axis = row ? 0 : 1;
  const free = () => {
    const rect = box.current?.getBoundingClientRect();
    return Math.max(1, (row ? (rect?.width ?? 0) : (rect?.height ?? 0)) - state.gap * (node.children.length - 1));
  };
  /** The shares after moving splitter `i` by `delta` shares, so neither neighbour is left under its minimum. */
  const moved = (i: number, delta: number, from: number[]) => {
    const room = free();
    const lo = state.mins[nodeId(node.children[i])][axis] / room - from[i];
    const hi = from[i + 1] - state.mins[nodeId(node.children[i + 1])][axis] / room;
    const by = lo > hi ? 0 : Math.max(lo, Math.min(hi, delta));
    return from.map((share, k) => (k === i ? share + by : k === i + 1 ? share - by : share));
  };
  const keep = (next: number[]) => {
    setLive(next);
    void layoutResize(node.id, next).finally(() => setLive(null));
  };
  // The splitter held: where the pointer was, the room there is and the shares then.
  const held = useRef<{ start: number; room: number; from: number[]; latest: number[] } | null>(null);
  const at = (event: PointerEvent<HTMLDivElement>) => (row ? event.clientX : event.clientY);
  const hold = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    held.current = { start: at(event), room: free(), from: node.sizes, latest: node.sizes };
  };
  const drag = (i: number, event: PointerEvent<HTMLDivElement>) => {
    const h = held.current;
    if (!h) return;
    h.latest = moved(i, (at(event) - h.start) / h.room, h.from);
    setLive(h.latest);
  };
  const release = () => {
    const h = held.current;
    held.current = null;
    if (h && h.latest !== h.from) keep(h.latest);
    else setLive(null);
  };
  const keys = (i: number, event: KeyboardEvent) => {
    const step = { ArrowLeft: row ? -1 : 0, ArrowRight: row ? 1 : 0, ArrowUp: row ? 0 : -1, ArrowDown: row ? 0 : 1 }[event.key];
    if (!step) return;
    event.preventDefault();
    keep(moved(i, step * STEP, node.sizes));
  };
  return (
    <div ref={box} className={`vj-split ${node.dir}`}>
      {node.children.map((child, i) => {
        const [w, h] = state.mins[nodeId(child)];
        return (
          <Fragment key={nodeId(child)}>
            {i > 0 && (
              <div className="vj-sash" role="separator" tabIndex={0} aria-orientation={row ? "vertical" : "horizontal"} aria-label={t("vj.dock.resize")}
                style={{ flex: `0 0 ${state.gap}px` }} onPointerDown={hold} onPointerMove={(event) => drag(i - 1, event)} onPointerUp={release} onPointerCancel={release} onKeyDown={(event) => keys(i - 1, event)} />
            )}
            <div className="vj-dn" style={{ flex: `${sizes[i] * 1000} 1 0px`, minWidth: w, minHeight: h }}>
              <Tree node={child} />
            </div>
          </Fragment>
        );
      })}
    </div>
  );
}

function Tree({ node }: { node: DockNode }) {
  if (node.type === "split") return <Split node={node} />;
  return node.type === "tabs" ? <Group node={node} /> : <DeckSlot />;
}

/**
 * The panels of the screen where the layout puts them: `deck` is the fixed leaf, `renderPanel` draws the panel of an id.
 * A window too small for every panel's minimum scrolls instead of crushing them.
 */
export function Dock({ deck, renderPanel }: { deck: ReactNode; renderPanel: (id: string) => ReactNode }) {
  const state = useVj((vj) => vj.layout);
  if (!state) return null;
  const [w, h] = state.mins[nodeId(state.current.root)];
  return (
    <Dock$.Provider value={{ state, deck, renderPanel }}>
      <div className="vj-dock">
        <div className="vj-dock-in" style={{ minWidth: w, minHeight: h }}>
          <Tree node={state.current.root} />
        </div>
      </div>
    </Dock$.Provider>
  );
}
