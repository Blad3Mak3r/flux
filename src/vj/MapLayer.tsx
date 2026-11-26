// Map mode, as in Resolume's "Edit MIDI / OSC": an outline over every control that can be mapped, a click on it waits
// for a MIDI control or an OSC message, and a right click clears it. The controls say what they are with `data-map`
// (see `mappable`); the mappings are the backend's, so this only reads them and forwards the click.

import { useEffect, useState } from "react";
import { controlGet } from "../control/api";
import type { ControlView, LinkState } from "../control/api";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { VideoAsset, VjGrid, VjTarget } from "../project/types";
import { useDoc } from "../state/store";
import { linkKnob, setMapMode, clearMapping, pickTarget, useMap } from "./mapping";
import { linkable, mappingsByTarget, sourceLabel, targetKey, targetLabel } from "./mapModel";

const NO_VIDEOS: VideoAsset[] = [];

interface Box {
  key: string;
  target: VjTarget;
  x: number;
  y: number;
  w: number;
  h: number;
}

/** What of `el` shows: its box cut by every ancestor that clips it, or `null` when nothing shows. */
function visible(el: Element): { x: number; y: number; w: number; h: number } | null {
  const box = el.getBoundingClientRect();
  let [left, top, right, bottom] = [box.left, box.top, box.right, box.bottom];
  for (let parent = el.parentElement; parent && parent !== document.body; parent = parent.parentElement) {
    const style = getComputedStyle(parent);
    if (style.overflowX === "visible" && style.overflowY === "visible") continue;
    const clip = parent.getBoundingClientRect();
    left = Math.max(left, clip.left);
    top = Math.max(top, clip.top);
    right = Math.min(right, clip.right);
    bottom = Math.min(bottom, clip.bottom);
  }
  return right - left > 3 && bottom - top > 3 ? { x: left, y: top, w: right - left, h: bottom - top } : null;
}

function measure(): Box[] {
  const found: Box[] = [];
  document.querySelectorAll("[data-map]").forEach((el, i) => {
    const area = visible(el);
    if (!area) return;
    try {
      const target = JSON.parse(el.getAttribute("data-map") ?? "") as VjTarget;
      found.push({ key: `${targetKey(target)}#${i}`, target, ...area });
    } catch {
      // A malformed attribute is a bug of the control; the overlay skips it.
    }
  });
  return found;
}

const same = (a: Box[], b: Box[]) =>
  a.length === b.length && a.every((box, i) => box.key === b[i].key && box.x === b[i].x && box.y === b[i].y && box.w === b[i].w && box.h === b[i].h);

/** A short tag for a source, for a box too small for its whole name. */
function shortSource(label: string): string {
  const [head] = label.split(" · ");
  return head.startsWith("/") ? "OSC" : head.replace("Note ", "N").replace("CC ", "CC").replace("Pitch bend", "PB");
}

/** The overlays of the controls: only while the map mode is on. */
export function MapLayer({ grid }: { grid: VjGrid }) {
  const t = useT();
  const on = useMap((state) => state.on);
  const learn = useMap((state) => state.learn);
  const linking = useMap((state) => state.linking);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const [boxes, setBoxes] = useState<Box[]>([]);
  useEffect(() => {
    if (!on) return;
    const run = () => setBoxes((old) => {
      const next = measure();
      return same(old, next) ? old : next;
    });
    run();
    const timer = window.setInterval(run, 250);
    window.addEventListener("resize", run);
    window.addEventListener("scroll", run, true);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("resize", run);
      window.removeEventListener("scroll", run, true);
    };
  }, [on]);
  if (!on) return null;
  const mapped = mappingsByTarget(grid);
  const learning = learn ? targetKey(learn) : null;
  const linked = linking === null ? new Set<string>() : new Set((grid.dashboard?.[linking]?.links ?? []).map((link) => targetKey(link.target)));
  return (
    <div className="vj-map" aria-label={t("vj.map.layer")}>
      {boxes.map((box) => {
        const key = targetKey(box.target);
        const list = mapped.get(key) ?? [];
        const source = list[0] ? sourceLabel(list[0].source) : null;
        const name = targetLabel(t, box.target, grid, videos);
        const waiting = learning === key;
        const off = linking !== null && !linkable(box.target);
        const knob = linking !== null && linked.has(key);
        const wide = box.w >= 84;
        const narrow = box.w < 40 || box.h < 18;
        const tag = waiting ? t("vj.map.learn") : knob ? t("vj.map.linked") : source ? (wide ? source : shortSource(source)) : null;
        const title = waiting ? t("vj.map.waitingFor", { target: name })
          : source ? `${name}: ${list.map((m) => sourceLabel(m.source)).join(", ")}. ${t("vj.map.clearNote")}`
          : `${name}: ${t("vj.map.unmapped")}`;
        return (
          <button key={box.key} type="button"
            className={`vj-map-o${waiting ? " learning" : ""}${source ? " mapped" : ""}${knob ? " linked" : ""}${off ? " off" : ""}${narrow ? " narrow" : ""}`}
            style={{ left: box.x, top: box.y, width: box.w, height: box.h }}
            title={title} aria-label={title} aria-pressed={waiting}
            disabled={off}
            onClick={() => pickTarget(box.target)}
            onContextMenu={(event) => {
              event.preventDefault();
              if (source) clearMapping(box.target);
            }}>
            {tag && !narrow && <span className="vj-map-t">{tag}{list.length > 1 && !waiting ? ` +${list.length - 1}` : ""}</span>}
          </button>
        );
      })}
    </div>
  );
}

const stateText = (link: LinkState | undefined): string => (link ? link.state : "off");

/** The strip over the grid while the mode is on: what it waits for, what listens, and the way out. */
export function MapBar({ grid }: { grid: VjGrid }) {
  const t = useT();
  const learn = useMap((state) => state.learn);
  const linking = useMap((state) => state.linking);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const [inputs, setInputs] = useState<Pick<ControlView, "midi" | "osc"> | null>(null);
  useEffect(() => {
    let alive = true;
    void controlGet().then((view) => alive && setInputs(view)).catch(() => undefined);
    return () => { alive = false; };
  }, []);
  const status = linking !== null
    ? t("vj.map.linkingKnob", { n: linking + 1 })
    : learn ? t("vj.map.waitingFor", { target: targetLabel(t, learn, grid, videos) }) : t("vj.map.hint");
  const lamp = (name: "midi" | "osc") => {
    const link = inputs?.[name];
    const state = stateText(link);
    return (
      <span className={`vj-map-in ${state}`} title={link && (link.state === "listening" || link.state === "error") ? link.detail : undefined}>
        <i aria-hidden="true" />{name.toUpperCase()} {t(`vj.map.state.${state}` as MessageKey)}
      </span>
    );
  };
  return (
    <div className="vj-mapbar" role="status">
      <strong>{t("vj.map.banner")}</strong>
      <span className="vj-mapbar-s">{status}</span>
      {lamp("midi")}
      {lamp("osc")}
      {linking !== null && <button type="button" className="vj-tb" onClick={() => linkKnob(null)}>{t("vj.map.linkDone")}</button>}
      <button type="button" className="vj-tb" onClick={() => setMapMode(false)}>{t("vj.map.exit")}<kbd>Esc</kbd></button>
    </div>
  );
}

