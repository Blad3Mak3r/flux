// The header of a layer: its name, mix, faders and the clip on air.

import { useState } from "react";
import type { MouseEvent } from "react";
import { InlineName } from "../components/InlineName";
import { Marquee } from "../components/Marquee";
import { ChevronDownIcon, ChevronRightIcon, CloseIcon, GlobeIcon, VideoIcon } from "../components/icons";
import { useT } from "../i18n";
import type { VjBlend, VjGrid, VjLayer, VjSide } from "../project/types";
import { useDoc } from "../state/store";
import { startDrag, useDropTarget } from "../state/pointerDrag";
import { openMenu } from "../state/ui";
import type { MenuItem } from "../state/ui";
import { edit, preview, selectLayer, stopLayer, useVj } from "./store";
import { clipName, layerName } from "./names";
import { groupItems } from "./menus";
import { mappable } from "./mapModel";
import { EffectBadge, carriesEffect, dropEffect } from "./EffectStack";
import { Select } from "../components/Select";
import { stripOf, Thumb } from "./VideoThumb";
import { NO_VIDEOS, AUTOPILOTS, DRAG_LAYER } from "./shared";
import { blendOptions, transitionOptions, transitionValue, transitionEdit } from "./LayerOptions";
import { Fader, PlayTime } from "./LayerFader";

/** The longest transition, in seconds, as the backend keeps it. */
const MAX_TRANSITION = 10;

const RESET_LEVEL = { layerMaster: "master", layerOpacity: "opacity", layerVolume: "volume", layerTransition: "transition" } as const;

export function LayerHeader({ grid, layer }: { grid: VjGrid; layer: VjLayer }) {
  const t = useT();
  const playingClip = useVj((vj) => vj.play.playing.find((p) => p.layer === layer.id)?.clip);
  const transition = useVj((vj) => vj.play.playing.find((p) => p.layer === layer.id)?.transition);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const thumbs = useDoc((doc) => doc.thumbs);
  const [renaming, setRenaming] = useState(false);
  const [over, setOver] = useState(false);
  const picked = useVj((vj) => vj.picked.includes(layer.id));
  const name = layerName(t, grid, layer);
  const clip = playingClip === undefined ? undefined : layer.clips?.find((c) => c.id === playingClip);
  const level = (op: "layerMaster" | "layerOpacity" | "layerVolume" | "layerTransition", scale = 1) => ({
    onMove: (value: number) => preview({ op, layer: layer.id, value: value * scale }),
    onSet: (value: number) => void edit({ op, layer: layer.id, value: value * scale }),
    onReset: () => void edit({ op: "layerLevelReset", layer: layer.id, level: RESET_LEVEL[op] }),
  });
  const side = (s: VjSide) => void edit({ op: "layerCrossfade", layer: layer.id, side: layer.crossfade === s ? null : s });
  const fold = () => void edit({ op: "layerCollapse", layer: layer.id, on: !layer.collapsed });
  const at = grid.layers.indexOf(layer);
  const menu: MenuItem[] = [
    { label: t("vj.layer.rename"), run: () => setRenaming(true) },
    { label: t(layer.collapsed ? "vj.layer.expand" : "vj.layer.collapse"), run: fold },
    { label: t("vj.layer.up"), disabled: at === grid.layers.length - 1, run: () => void edit({ op: "moveLayer", layer: layer.id, to: at + 1 }) },
    { label: t("vj.layer.down"), disabled: at === 0, run: () => void edit({ op: "moveLayer", layer: layer.id, to: at - 1 }) },
    "-",
    { heading: t("vj.layer.autopilot") },
    ...AUTOPILOTS.map((mode) => ({
      label: t(`vj.autopilot.${mode}`), checked: (layer.autopilot ?? "off") === mode,
      run: () => void edit({ op: "layerAutopilot", layer: layer.id, mode }),
    })),
    "-",
    { label: t("vj.layer.remove"), disabled: grid.layers.length === 1, run: () => void edit({ op: "removeLayer", layer: layer.id }) },
  ];
  const bs = (
    <div className="vj-bs">
      <button type="button" className={layer.bypass ? "on bypass" : ""} {...mappable({ kind: "layerBypass", layer: layer.id })} aria-pressed={!!layer.bypass} title={t("vj.layer.bypass")}
        onClick={() => void edit({ op: "layerBypass", layer: layer.id, on: !layer.bypass })}>B</button>
      <button type="button" className={layer.solo ? "on solo" : ""} {...mappable({ kind: "layerSolo", layer: layer.id })} aria-pressed={!!layer.solo} title={t("vj.layer.solo")}
        onClick={() => void edit({ op: "layerSolo", layer: layer.id, on: !layer.solo })}>S</button>
    </div>
  );
  const nameRow = (
      <div className="vj-lname" onPointerDown={(event) => startDrag(event, { [DRAG_LAYER]: layer.id }, name)}>
        <button type="button" className="vj-fold" aria-expanded={!layer.collapsed}
          title={t(layer.collapsed ? "vj.layer.expand" : "vj.layer.collapse")} aria-label={t(layer.collapsed ? "vj.layer.expand" : "vj.layer.collapse")}
          onClick={(event) => { event.stopPropagation(); fold(); }}>
          {layer.collapsed ? <ChevronRightIcon /> : <ChevronDownIcon />}
        </button>
        <span className="vj-num">{at + 1}</span>
        {renaming ? (
          <InlineName value={layer.name ?? ""} label={t("vj.layer.rename")} onDone={() => setRenaming(false)}
            onCommit={(text) => void edit({ op: "renameLayer", layer: layer.id, name: text })} />
        ) : (
          <Marquee className="vj-nm" title={name} onDoubleClick={() => setRenaming(true)}>{name}</Marquee>
        )}
        {layer.global && <span className="vj-global" title={t("vj.layer.global")}><GlobeIcon /></span>}
        <EffectBadge count={layer.effects?.length ?? 0} />
        {layer.autopilot && layer.autopilot !== "off" && (
          <span className="vj-ap" title={`${t("vj.layer.autopilot")}: ${t(`vj.autopilot.${layer.autopilot}`)}`}>
            AP {t(`vj.autopilot.short.${layer.autopilot}`)}
          </span>
        )}
        <Marquee className={`vj-clipname${clip ? "" : " idle"}`}>{clip ? clipName(t, clip, videos) : t("vj.layer.noClip")}</Marquee>
        <span className="vj-ab" role="group" aria-label={t("vj.layer.side")}>
          {(["a", "b"] as VjSide[]).map((s) => (
            <button key={s} type="button" className={layer.crossfade === s ? "on" : ""} aria-pressed={layer.crossfade === s}
              onClick={(event) => { event.stopPropagation(); side(s); }}>{s.toUpperCase()}</button>
          ))}
        </span>
      </div>
  );
  const className = `vj-lh${playingClip !== undefined ? " playing" : ""}${layer.global ? " global" : ""}${over ? " over" : ""}${picked ? " picked" : ""}`;
  // An effect dropped on the header goes to the layer's stack; a layer dropped on it takes its place and its group.
  const dropping = useDropTarget({
    accepts: (data) => DRAG_LAYER in data || carriesEffect(data, { layer: layer.id }),
    over: setOver,
    drop: (data) => {
      const dragged = data[DRAG_LAYER];
      if (!dragged) dropEffect(data, { layer: layer.id });
      else if (dragged !== layer.id) void edit({ op: "layerGroup", layer: dragged, group: layer.group ?? null, to: at });
    },
  });
  const onContextMenu = (event: MouseEvent) => {
    event.preventDefault();
    if (!useVj.getState().picked.includes(layer.id)) selectLayer(layer.id);
    const items = [...menu, "-" as const, { heading: t("vj.group.menu") }, ...groupItems(t, grid, layer)];
    openMenu({ x: event.clientX, y: event.clientY, items });
  };
  const select = (event: MouseEvent) => selectLayer(layer.id, event.ctrlKey || event.metaKey);
  if (layer.collapsed) {
    // Folded as in Resolume: one thin row with the name, the clip on air, bypass, solo and the side.
    return (
      <div className={`${className} folded`} onClick={select} onContextMenu={onContextMenu} ref={dropping}>
        <button type="button" className="vj-eject" {...mappable({ kind: "layerStop", layer: layer.id })} title={t("vj.layer.eject")} aria-label={t("vj.layer.eject")}
          onClick={(event) => { event.stopPropagation(); void stopLayer(layer.id); }}>
          <CloseIcon />
        </button>
        {bs}
        {nameRow}
      </div>
    );
  }
  return (
    <div
      className={className}
      onClick={select}
      onContextMenu={onContextMenu}
      ref={dropping}
    >
      <button type="button" className="vj-eject" {...mappable({ kind: "layerStop", layer: layer.id })} title={t("vj.layer.eject")} aria-label={t("vj.layer.eject")}
        onClick={(event) => { event.stopPropagation(); void stopLayer(layer.id); }}>
        <CloseIcon />
      </button>
      <div className="vj-bsb">
        {bs}
        <Select className="vj-select" value={layer.blend ?? "alpha"} title={t("vj.layer.blend")} aria-label={t("vj.layer.blend")}
          onChange={(value) => void edit({ op: "layerBlend", layer: layer.id, blend: value as VjBlend })}>
          {blendOptions(t)}
        </Select>
      </div>
      <div className="vj-faders">
        <Fader letter="M" map={{ kind: "layerMaster", layer: layer.id }} label={t("vj.fader.master")} value={layer.master ?? 1} {...level("layerMaster")}
          modulated={layer.modulation?.master ? { layer: layer.id, field: "master" } : undefined} />
        <Fader letter="A" map={{ kind: "layerVolume", layer: layer.id }} label={t("vj.fader.audio")} value={layer.volume ?? 1} {...level("layerVolume")}
          modulated={layer.modulation?.volume ? { layer: layer.id, field: "volume" } : undefined} />
        <Fader letter="V" map={{ kind: "layerOpacity", layer: layer.id }} label={t("vj.fader.video")} value={layer.opacity ?? 1} {...level("layerOpacity")}
          modulated={layer.modulation?.opacity ? { layer: layer.id, field: "opacity" } : undefined} />
      </div>
      <Thumb className={`vj-now${clip ? "" : " empty"}`} strip={stripOf(thumbs, clip)} generator={clip?.generator}>
        {clip ? <PlayTime layer={layer.id} countdown="header" /> : <span className="vj-nonow" aria-hidden="true"><VideoIcon /></span>}
        {transition != null && <span className="vj-tbar"><i style={{ width: `${transition * 100}%` }} /></span>}
      </Thumb>
      <div className="vj-tcol">
        <Fader letter="T" map={{ kind: "layerTransition", layer: layer.id }} label={t("vj.fader.transition")} value={(layer.transition ?? 0) / MAX_TRANSITION}
          progress={transition} {...level("layerTransition", MAX_TRANSITION)} />
        <Select className="vj-select short" value={transitionValue(layer)} title={t("vj.layer.transitionEffect")}
          aria-label={t("vj.layer.transitionEffect")}
          onChange={(value) => void edit(transitionEdit(layer, value))}>
          {transitionOptions(t)}
        </Select>
      </div>
      {nameRow}
    </div>
  );
}
