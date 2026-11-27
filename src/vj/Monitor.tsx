// A monitor panel: a picture the backend draws, of the composition, of the clip selected, or of one layer or group,
// with a footer that names it and a gear that changes what it shows. Only the monitor in sight asks for pictures
// (a monitor in a hidden tab is not drawn), so the open ones cost what they show.

import { useEffect, useRef } from "react";
import type { RefObject } from "react";
import { Marquee } from "../components/Marquee";
import { GearIcon, PlayIcon } from "../components/icons";
import { useT } from "../i18n";
import type { Translate } from "../i18n";
import type { VideoAsset, VjGrid } from "../project/types";
import { useDoc } from "../state/store";
import { openMenu } from "../state/ui";
import type { MenuItem } from "../state/ui";
import { useComposition } from "../video/mapping";
import { kindTitle } from "./Dock";
import type { MonitorKind } from "./api";
import { CompositionFx, useCompositionDrop } from "./EffectStack";
import { clipName, groupName, layerName } from "./names";
import { toggleDisable, useOutput } from "./output";
import { bindMonitor, bindOutput, bindPreview, launch, layoutSetMonitor, useVj } from "./store";

const NO_VIDEOS: VideoAsset[] = [];
type Draw = (picture: ImageData | null) => void;

/** A function that paints the backend's pictures in `canvas`, growing it to their size. */
function painter(canvas: RefObject<HTMLCanvasElement | null>): Draw {
  return (picture) => {
    const target = canvas.current;
    const context = target?.getContext("2d");
    if (!target || !context) return;
    if (picture && (target.width !== picture.width || target.height !== picture.height)) {
      target.width = picture.width;
      target.height = picture.height;
    }
    if (picture) context.putImageData(picture, 0, 0);
    else context.clearRect(0, 0, target.width, target.height);
  };
}

/** What a monitor draws, as the backend names it, and the picture's source for it; `null` when there is nothing to draw. */
interface Source {
  key: string;
  start: ((draw: Draw) => () => void) | null;
}

/** The gear: what the monitor shows, and for a layer or a group, the selection it follows or the one it is fixed on. */
function gearItems(t: Translate, grid: VjGrid, id: string, kind: MonitorKind): MenuItem[] {
  const set = (next: MonitorKind) => () => void layoutSetMonitor(id, next);
  const kinds: MonitorKind[] = [{ type: "composition" }, { type: "preview" }, { type: "layer", layer: null }, { type: "group", group: null }];
  const items: MenuItem[] = [
    { heading: t("vj.monitor.show") },
    ...kinds.map((next) => ({ label: kindTitle(t, next), checked: next.type === kind.type, run: set(next) })),
  ];
  if (kind.type === "layer") {
    items.push("-", { label: t("vj.monitor.follow"), checked: kind.layer === null, run: set({ type: "layer", layer: null }) });
    for (const layer of [...grid.layers].reverse()) {
      items.push({ label: layerName(t, grid, layer), checked: kind.layer === layer.id, run: set({ type: "layer", layer: layer.id }) });
    }
  }
  if (kind.type === "group") {
    items.push("-", { label: t("vj.monitor.follow"), checked: kind.group === null, run: set({ type: "group", group: null }) });
    for (const group of grid.groups ?? []) {
      items.push({ label: groupName(t, grid, group), checked: kind.group === group.id, run: set({ type: "group", group: group.id }) });
    }
  }
  return items;
}

/** The monitor `id` of the layout, showing `kind`. */
export function Monitor({ grid, id, kind }: { grid: VjGrid; id: string; kind: MonitorKind }) {
  const t = useT();
  const composition = useCompositionDrop();
  const size = useComposition();
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const live = useVj((vj) => vj.play.playing.some((p) => p.level > 0));
  const disabled = useOutput((output) => output.disabled);
  const testCard = useOutput((output) => output.testCard);
  const selectedClip = useVj((vj) => vj.clip);
  const selectedLayer = useVj((vj) => vj.layer);
  const selectedGroup = useVj((vj) => vj.group);
  const canvas = useRef<HTMLCanvasElement>(null);

  const clip = kind.type === "preview"
    ? grid.layers.flatMap((layer) => (layer.clips ?? []).map((c) => ({ layer, clip: c }))).find((x) => x.clip.id === selectedClip)
    : undefined;
  const layer = kind.type === "layer" ? grid.layers.find((l) => l.id === (kind.layer ?? selectedLayer)) : undefined;
  const group = kind.type === "group" ? grid.groups?.find((g) => g.id === (kind.group ?? selectedGroup)) : undefined;
  const source: Source =
    kind.type === "composition" ? { key: "composition", start: bindOutput }
    : kind.type === "preview" ? { key: `preview ${clip?.clip.id}`, start: clip ? (draw) => bindPreview(clip.clip.id, draw) : null }
    : kind.type === "layer" ? { key: `layer ${layer?.id}`, start: layer ? (draw) => bindMonitor("layer", layer.id, draw) : null }
    : { key: `group ${group?.id}`, start: group ? (draw) => bindMonitor("group", group.id, draw) : null };
  useEffect(() => source.start?.(painter(canvas)), [source.key]);

  const px = `${size.width}×${size.height}`;
  const label =
    kind.type === "composition" ? `${t("vj.composition")} · ${px}`
    : kind.type === "preview" ? (clip ? `${t("vj.preview")} · ${clipName(t, clip.clip, videos)}` : t("vj.preview"))
    : kind.type === "layer" ? (layer ? `${layerName(t, grid, layer)} · ${px}` : t("vj.inspector.layer"))
    : group ? `${groupName(t, grid, group)} · ${px}` : t("vj.inspector.group");
  const empty = kind.type === "preview" ? "vj.previewEmpty" : kind.type === "layer" ? "vj.inspector.noLayer" : "vj.inspector.noGroup";

  return (
    <>
      <div className={`vj-pb vj-mon-wrap${kind.type === "composition" && composition.over ? " over" : ""}`} ref={kind.type === "composition" ? composition.ref : undefined}>
        <div className="vj-mon" data-disabled={kind.type === "composition" && disabled}>
          {source.start ? <canvas ref={canvas} className="vj-out" /> : <p className="vj-empty">{t(empty)}</p>}
          {kind.type === "composition" && disabled && <p className="vj-mon-off" role="status">{t("vj.output.disabledNote")}</p>}
        </div>
        {clip && (
          <div className="vj-acts">
            <button type="button" className="button primary" onClick={() => void launch(clip.clip.id, clip.layer.id)}>
              <PlayIcon />{t("vj.launchOn", { layer: layerName(t, grid, clip.layer) })}
            </button>
          </div>
        )}
      </div>
      <footer className="vj-mfoot">
        <Marquee className="vj-mlabel" title={label}>{label}</Marquee>
        {kind.type === "composition" && (
          <>
            <CompositionFx grid={grid} />
            {disabled && <button type="button" className="vj-pill off" title={t("vj.output.reenable")} onClick={toggleDisable}><i />{t("vj.output.disabled")}</button>}
            {testCard && !disabled && <span className="vj-pill card"><i />{t("vj.output.testCard")}</span>}
            {live && <span className="vj-pill"><i />{t("vj.onAir")}</span>}
          </>
        )}
        <button type="button" className="vj-tb vj-gear" title={t("vj.monitor.settings")} aria-label={t("vj.monitor.settings")} aria-haspopup="menu"
          onClick={(event) => {
            const at = event.currentTarget.getBoundingClientRect();
            openMenu({ x: at.left, y: at.bottom, items: gearItems(t, grid, id, kind) });
          }}>
          <GearIcon />
        </button>
      </footer>
    </>
  );
}
