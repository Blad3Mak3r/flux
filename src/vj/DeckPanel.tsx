// The grid itself: its decks, the crossfader, the groups and the rows of layers.

import { useState } from "react";
import type { CSSProperties, MouseEvent, PointerEvent, ReactNode } from "react";
import { InlineName } from "../components/InlineName";
import { Marquee } from "../components/Marquee";
import { ChevronDownIcon, ChevronRightIcon, CloseIcon, GlobeIcon, PlayIcon, PlusIcon, StopIcon } from "../components/icons";
import { useT } from "../i18n";
import type { VjBlend, VjGroup, VjDeck, VjGrid, VjSide } from "../project/types";
import { useDropTarget } from "../state/pointerDrag";
import { openMenu } from "../state/ui";
import type { MenuItem } from "../state/ui";
import { edit, launchGroupColumn, moveCrossfader, resetCrossfader, preview, selectGroup, layoutSetClipSize, stopLayer, useVj } from "./store";
import type { ClipSize } from "./api";
import { groupName, layerName } from "./names";
import { MapBar } from "./MapLayer";
import { useMap } from "./mapping";
import { mappable } from "./mapModel";
import { stepDeck } from "./menus";
import { Segments } from "./controls";
import { CompositionFx, EffectBadge, carriesEffect, dropEffect, useCompositionDrop } from "./EffectStack";
import { AnalysisMeter, Tempo, TimecodeClock } from "./CompositionBar";
import { Select } from "../components/Select";
import { DRAG_LAYER } from "./shared";
import { blendOptions } from "./LayerOptions";
import { Fader } from "./LayerFader";
import { LayerHeader } from "./LayerHeader";
import { Cell, ColumnHead } from "./Cell";

const CLIP_SIZES: ClipSize[] = ["small", "medium", "large"];

function DeckTab({ deck, at, shown, only }: { deck: VjDeck; at: number; shown: boolean; only: boolean }) {
  const t = useT();
  const [renaming, setRenaming] = useState(false);
  const name = deck.name ?? t("vj.deck.default", { n: at + 1 });
  return (
    <button
      type="button"
      role="tab"
      aria-selected={shown}
      className={`vj-deck${shown ? " on" : ""}`}
      {...mappable({ kind: "deckSelect", deck: deck.id })}
      onClick={() => { if (!shown) void edit({ op: "selectDeck", deck: deck.id }); }}
      onDoubleClick={() => setRenaming(true)}
      onContextMenu={(event) => {
        event.preventDefault();
        openMenu({ x: event.clientX, y: event.clientY, items: [
          { label: t("vj.deck.rename"), run: () => setRenaming(true) },
          { label: t("vj.deck.remove"), disabled: only, run: () => void edit({ op: "removeDeck", deck: deck.id }) },
        ] });
      }}
    >
      {renaming ? (
        <InlineName value={deck.name ?? ""} label={t("vj.deck.rename")} onDone={() => setRenaming(false)}
          onCommit={(text) => void edit({ op: "renameDeck", deck: deck.id, name: text })} />
      ) : <Marquee className="vj-dname">{name}</Marquee>}
      <span className="vj-count">{deck.columns.length}</span>
    </button>
  );
}

function Crossfader() {
  const t = useT();
  const position = useVj((vj) => vj.play.crossfader);
  const [held, setHeld] = useState<number | null>(null);
  const shown = held ?? position;
  const at = (event: PointerEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return Math.min(1, Math.max(0, (event.clientX - box.left) / box.width));
  };
  const move = (v: number) => {
    setHeld(v);
    void moveCrossfader(v);
  };
  return (
    <div
      className="vj-xt"
      {...mappable({ kind: "crossfader" })}
      role="slider"
      tabIndex={0}
      aria-label="Crossfader"
      title={t("vj.fader.reset")}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(shown * 100)}
      onContextMenu={(event) => { event.preventDefault(); setHeld(null); void resetCrossfader(); }}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        move(at(event));
      }}
      onPointerMove={(event) => { if (held !== null) move(at(event)); }}
      onPointerUp={() => setHeld(null)}
      onPointerCancel={() => setHeld(null)}
      onDoubleClick={() => void resetCrossfader()}
    >
      <span className="vj-tick" style={{ left: 0 }} />
      <span className="vj-tick mid" />
      <span className="vj-tick" style={{ left: "100%" }} />
      <b style={{ left: `${shown * 100}%` }} />
    </div>
  );
}

/** The header row of a group: its switches, blend and faders, and a trigger per column that launches it on the group's layers only. */
function GroupRow({ grid, group, deck }: { grid: VjGrid; group: VjGroup; deck: VjDeck }) {
  const t = useT();
  const [renaming, setRenaming] = useState(false);
  const [over, setOver] = useState(false);
  const selected = useVj((vj) => vj.tab === "group" && vj.group === group.id);
  const name = groupName(t, grid, group);
  const count = grid.layers.filter((layer) => layer.group === group.id).length;
  const at = (grid.groups ?? []).indexOf(group);
  const level = (op: "groupMaster" | "groupOpacity") => ({
    onMove: (value: number) => preview({ op, group: group.id, value }),
    onSet: (value: number) => void edit({ op, group: group.id, value }),
    onReset: () => void edit({ op: "groupLevelReset", group: group.id, level: op === "groupMaster" ? "master" : "opacity" }),
  });
  const fold = () => void edit({ op: "groupCollapse", group: group.id, on: !group.collapsed });
  const ungroup = () => void edit({ op: "ungroup", group: group.id });
  const foldKey = group.collapsed ? "vj.group.expand" : "vj.group.collapse";
  const menu: MenuItem[] = [
    { label: t("vj.group.rename"), run: () => setRenaming(true) },
    { label: t(foldKey), run: fold },
    "-",
    { label: t("vj.group.ungroup"), run: ungroup },
  ];
  const dropping = useDropTarget({
    accepts: (data) => DRAG_LAYER in data || carriesEffect(data, { group: group.id }),
    over: setOver,
    drop: (data) => {
      const dragged = data[DRAG_LAYER];
      if (!dragged) dropEffect(data, { group: group.id });
      else void edit({ op: "layerGroup", layer: dragged, group: group.id, to: null });
    },
  });
  const bs = (
    <div className="vj-bs">
      <button type="button" className={group.bypass ? "on bypass" : ""} {...mappable({ kind: "groupBypass", group: group.id })} aria-pressed={!!group.bypass} title={t("vj.group.bypass")}
        onClick={(event) => { event.stopPropagation(); void edit({ op: "groupBypass", group: group.id, on: !group.bypass }); }}>B</button>
      <button type="button" className={group.solo ? "on solo" : ""} {...mappable({ kind: "groupSolo", group: group.id })} aria-pressed={!!group.solo} title={t("vj.group.solo")}
        onClick={(event) => { event.stopPropagation(); void edit({ op: "groupSolo", group: group.id, on: !group.solo }); }}>S</button>
    </div>
  );
  const nameRow = (
    <div className="vj-lname">
      <button type="button" className="vj-fold" aria-expanded={!group.collapsed} title={t(foldKey)} aria-label={t(foldKey)}
        onClick={(event) => { event.stopPropagation(); fold(); }}>
        {group.collapsed ? <ChevronRightIcon /> : <ChevronDownIcon />}
      </button>
      <span className="vj-num">G{at + 1}</span>
      {renaming ? (
        <InlineName value={group.name ?? ""} label={t("vj.group.rename")} onDone={() => setRenaming(false)}
          onCommit={(text) => void edit({ op: "renameGroup", group: group.id, name: text })} />
      ) : (
        <Marquee className="vj-nm" title={name} onDoubleClick={() => setRenaming(true)}>{name}</Marquee>
      )}
      <EffectBadge count={group.effects?.length ?? 0} />
      <span className="vj-clipname idle">{t(count === 1 ? "vj.group.layer" : "vj.group.layers", { n: count })}</span>
    </div>
  );
  const className = `vj-lh group${group.collapsed ? " folded" : ""}${over ? " over" : ""}${selected ? " selected" : ""}`;
  const onContextMenu = (event: MouseEvent) => {
    event.preventDefault();
    openMenu({ x: event.clientX, y: event.clientY, items: menu });
  };
  return (
    <div className={`vj-row group${group.collapsed ? " folded" : ""}`}>
      {group.collapsed ? (
        <div className={className} onClick={() => selectGroup(group.id)} onContextMenu={onContextMenu} ref={dropping}>
          {bs}
          {nameRow}
        </div>
      ) : (
        <div className={className} onClick={() => selectGroup(group.id)} onContextMenu={onContextMenu} ref={dropping}>
          <button type="button" className="vj-eject" title={t("vj.group.ungroup")} aria-label={t("vj.group.ungroup")}
            onClick={(event) => { event.stopPropagation(); ungroup(); }}>
            <CloseIcon />
          </button>
          <div className="vj-bsb">
            {bs}
            <Select className="vj-select" value={group.blend ?? "alpha"} title={t("vj.group.blend")} aria-label={t("vj.group.blend")}
              onClick={(event) => event.stopPropagation()}
              onChange={(value) => void edit({ op: "groupBlend", group: group.id, blend: value as VjBlend })}>
              {blendOptions(t)}
            </Select>
          </div>
          <div className="vj-faders two">
            <Fader letter="M" map={{ kind: "groupMaster", group: group.id }} label={t("vj.fader.master")} value={group.master ?? 1} {...level("groupMaster")} />
            <Fader letter="V" map={{ kind: "groupOpacity", group: group.id }} label={t("vj.fader.video")} value={group.opacity ?? 1} {...level("groupOpacity")} />
          </div>
          {nameRow}
        </div>
      )}
      {deck.columns.map((column, place) => (
        <button key={column.id} type="button" className="vj-gcell" title={t("vj.group.launch", { n: place + 1, name })} aria-label={t("vj.group.launch", { n: place + 1, name })}
          onPointerDown={(event) => { if (event.button === 0) void launchGroupColumn(group.id, column.id); }}>
          <PlayIcon />
        </button>
      ))}
    </div>
  );
}

export function DeckPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const deck = grid.decks.find((d) => d.id === grid.deck) ?? grid.decks[0];
  const layers = [...grid.layers].reverse();
  const lit = useVj((vj) => vj.play.column);
  const liveColumn = deck.columns.findIndex((column) => column.id === lit);
  const sideNames = (side: VjSide) => grid.layers.filter((l) => l.crossfade === side).map((l) => layerName(t, grid, l)).join(", ");
  const columns = { "--columns": deck.columns.length } as CSSProperties;
  const clipSize = useVj((vj) => vj.layout?.clipSize ?? "medium");
  // An effect dropped on the composition bar goes to the composition's stack.
  const composition = useCompositionDrop();
  const mapping = useMap((state) => state.on);
  return (
    <section className="vj-panel vj-deck-panel" aria-label={t("vj.grid")}>
      <header className="vj-ph">
        <h2>{t("vj.grid")}</h2>
        {mapping && <MapBar grid={grid} />}
        <div className="vj-tools">
          <Segments options={CLIP_SIZES} value={clipSize} label={(o) => t(`vj.clipSize.${o}.short`)} title={(o) => t(`vj.clipSize.${o}`)}
            onPick={(size) => void layoutSetClipSize(size)} />
          <button type="button" className="vj-tb" onClick={() => void edit({ op: "addLayer", global: false })}><PlusIcon />{t("vj.layer.add")}</button>
          <button type="button" className="vj-tb" onClick={() => void edit({ op: "addLayer", global: true })}><GlobeIcon />{t("vj.layer.addGlobal")}</button>
          <button type="button" className="vj-tb" onClick={() => void edit({ op: "addColumn" })}><PlusIcon />{t("vj.column.add")}</button>
        </div>
      </header>
      <div className="vj-pb">
        <div className="vj-grid" style={columns} data-size={clipSize}>
          <div className="vj-row head">
            <div className="vj-corner">{t("vj.summary", { layers: grid.layers.length, columns: deck.columns.length })}</div>
            {deck.columns.map((column, at) => <ColumnHead key={column.id} column={column} at={at} live={at === liveColumn} />)}
          </div>
          {layers.flatMap((layer, i) => {
            const group = grid.groups?.find((g) => g.id === layer.group);
            const rows: ReactNode[] = [];
            // A group's header row comes over its first layer; a folded group shows nothing else.
            if (group && layers[i - 1]?.group !== layer.group) rows.push(<GroupRow key={`group-${group.id}`} grid={grid} group={group} deck={deck} />);
            if (group?.collapsed) return rows;
            rows.push(
              <div key={layer.id} className={`vj-row${layer.global ? " global" : ""}${layer.collapsed ? " folded" : ""}${group ? " in-group" : ""}${i > 0 && layers[i - 1].global && !layer.global ? " split" : ""}`}>
                <LayerHeader grid={grid} layer={layer} />
                {deck.columns.map((column, at) => (
                  <Cell key={column.id} layer={layer} column={column} at={at} />
                ))}
              </div>,
            );
            return rows;
          })}
        </div>
        <div className="vj-decks">
          <div className="vj-xf" title={t("vj.crossfader.sides", { a: sideNames("a") || "—", b: sideNames("b") || "—" })}>
            <b>A</b>
            <Crossfader />
            <b>B</b>
          </div>
          <div className="vj-deck-tabs" role="tablist" aria-label={t("vj.decks")}>
            <button type="button" className="vj-deck step" {...mappable({ kind: "deckPrev" })} title={t("vj.menu.previousDeck")} aria-label={t("vj.menu.previousDeck")} onClick={() => stepDeck(-1)}>‹</button>
            <button type="button" className="vj-deck step" {...mappable({ kind: "deckNext" })} title={t("vj.menu.nextDeck")} aria-label={t("vj.menu.nextDeck")} onClick={() => stepDeck(1)}>›</button>
            {grid.decks.map((d, at) => <DeckTab key={d.id} deck={d} at={at} shown={d.id === deck.id} only={grid.decks.length === 1} />)}
            <button type="button" className="vj-deck add" title={t("vj.deck.add")} aria-label={t("vj.deck.add")}
              onClick={() => void edit({ op: "addDeck" })}><PlusIcon /></button>
          </div>
        </div>
      </div>
      <div className={`vj-comp${composition.over ? " over" : ""}`} ref={composition.ref}>
        <div className="vj-cl"><strong>{t("vj.composition")}</strong><AnalysisMeter /><CompositionFx grid={grid} /></div>
        <Tempo />
        <TimecodeClock grid={grid} />
        <button type="button" className="button" {...mappable({ kind: "stopAll" })} onClick={() => void stopLayer(null)}><StopIcon />{t("vj.stopAll")}</button>
      </div>
    </section>
  );
}
