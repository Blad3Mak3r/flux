// The dashboard of the composition: eight knobs, each moving one or more numbers of the show at once, every number
// between its own two limits. The backend keeps the knobs and moves what they are linked to (`vj_mapping.rs`); a
// knob can also be mapped to a MIDI control or an OSC address like any other number.

import { useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import { useT } from "../i18n";
import type { VideoAsset, VjGrid, VjKnob, VjLink } from "../project/types";
import { CloseIcon } from "../components/icons";
import { useDoc } from "../state/store";
import { Scrub } from "./controls";
import { useEffectCatalog } from "./effects";
import { linkKnob, useMap } from "./mapping";
import { mappable, targetKey, targetLabel, targetUnits } from "./mapModel";
import { edit, preview } from "./store";

const KNOBS = 8;
const NO_VIDEOS: VideoAsset[] = [];
/** How far a drag of the whole travel is, in pixels. */
const TRAVEL = 160;
const EMPTY: VjKnob = { value: 0 };

/** A rotary knob: its arc is the value, its pointer the same, and the number is printed under it. */
function Knob({ index, knob, picked, onPick }: { index: number; knob: VjKnob; picked: boolean; onPick: () => void }) {
  const t = useT();
  const [held, setHeld] = useState<{ y: number; from: number; v: number } | null>(null);
  const value = held?.v ?? knob.value;
  const label = t("vj.dashboard.knobNamed", { n: index + 1, name: knob.name ?? "" }).trim();
  const dragged = (event: PointerEvent<HTMLElement>, from: { y: number; from: number }) =>
    Math.min(1, Math.max(0, from.from + ((from.y - event.clientY) / TRAVEL) * (event.shiftKey ? 0.1 : 1)));
  const set = (v: number) => void edit({ op: "dashboardValue", knob: index, value: Math.min(1, Math.max(0, v)) });
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const step = event.key === "ArrowUp" || event.key === "ArrowRight" ? 1 : event.key === "ArrowDown" || event.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    set(knob.value + step * (event.shiftKey ? 0.01 : 0.05));
  };
  // The arc runs 270 degrees from the lower left; the pointer is at its end.
  const angle = (-225 + value * 270) * (Math.PI / 180);
  const at = (r: number, a: number) => [24 + r * Math.cos(a), 24 + r * Math.sin(a)];
  const arc = (to: number) => {
    const [x0, y0] = at(19, (-225 * Math.PI) / 180);
    const [x1, y1] = at(19, (to * Math.PI) / 180);
    return `M ${x0} ${y0} A 19 19 0 ${to + 225 > 180 ? 1 : 0} 1 ${x1} ${y1}`;
  };
  const [px, py] = at(5, angle);
  const [qx, qy] = at(13, angle);
  return (
    <div className={`vj-knob${picked ? " picked" : ""}${knob.links?.length ? " linked" : ""}`} {...mappable({ kind: "dashboard", knob: index })}>
      <div className="vj-knob-dial" role="slider" tabIndex={0} aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(value * 100)}
        title={`${label} ${Math.round(value * 100)}`}
        onKeyDown={onKeyDown}
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId);
          onPick();
          setHeld({ y: event.clientY, from: knob.value, v: knob.value });
        }}
        onPointerMove={(event) => {
          if (!held) return;
          const v = dragged(event, held);
          setHeld({ ...held, v });
          preview({ op: "dashboardValue", knob: index, value: v });
        }}
        onPointerUp={(event) => {
          if (!held) return;
          const v = dragged(event, held);
          setHeld(null);
          if (v !== knob.value) set(v);
        }}
        onPointerCancel={() => setHeld(null)}
        onDoubleClick={() => set(0)}>
        <svg viewBox="0 0 48 48" aria-hidden="true">
          <path className="vj-knob-rail" d={arc(45)} />
          {value > 0.002 && <path className="vj-knob-arc" d={arc(-225 + value * 270)} />}
          <circle className="vj-knob-cap" cx="24" cy="24" r="13" />
          <line className="vj-knob-ptr" x1={px} y1={py} x2={qx} y2={qy} />
        </svg>
      </div>
      <b className="vj-mono">{Math.round(value * 100)}</b>
      <span className="vj-knob-n">{knob.name || index + 1}</span>
    </div>
  );
}

function LinkRow({ grid, knob, link }: { grid: VjGrid; knob: number; link: VjLink }) {
  const t = useT();
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const catalog = useEffectCatalog((state) => state.catalog);
  const units = targetUnits(link.target, grid, catalog);
  const name = targetLabel(t, link.target, grid, videos);
  const set = (changed: Partial<Pick<VjLink, "min" | "max">>) =>
    void edit({ op: "dashboardLink", knob, target: link.target, min: link.min, max: link.max, ...changed });
  const scrub = (tag: string, label: string, key: "min" | "max") =>
    units && (
      <Scrub label={tag} name={`${name}: ${label}`} value={link[key]} shown={units.shown} reset={key === "min" ? units.min : units.max}
        unit={units.unit} decimals={units.decimals} perPixel={(units.max - units.min) / 160} step={(units.max - units.min) / 100}
        onMove={() => undefined} onSet={(v) => set({ [key]: Math.min(units.max, Math.max(units.min, v)) })} />
    );
  return (
    <li className="vj-link">
      <span className="vj-mp-tgt" title={name}>{name}</span>
      <span className="vj-mp-opt">
        {scrub("0", t("vj.dashboard.atMin"), "min")}
        {scrub("1", t("vj.dashboard.atMax"), "max")}
      </span>
      <button type="button" className="vj-mp-del" title={t("vj.dashboard.unlink")} aria-label={`${t("vj.dashboard.unlink")}: ${name}`}
        onClick={() => void edit({ op: "dashboardUnlink", knob, target: link.target })}><CloseIcon /></button>
    </li>
  );
}

/** The eight knobs and the links of the one picked. */
export function Dashboard({ grid }: { grid: VjGrid }) {
  const t = useT();
  const [picked, setPicked] = useState(0);
  const linking = useMap((state) => state.linking);
  const knob = grid.dashboard?.[picked] ?? EMPTY;
  const links = knob.links ?? [];
  const [draft, setDraft] = useState<string | null>(null);
  const rename = () => {
    if (draft !== null && draft !== (knob.name ?? "")) void edit({ op: "renameKnob", knob: picked, name: draft });
    setDraft(null);
  };
  return (
    <section className="vj-dash" aria-label={t("vj.dashboard.title")}>
      <h3 className="vj-sub">{t("vj.dashboard.title")}</h3>
      <p className="vj-dim vj-fx-empty">{t("vj.dashboard.note")}</p>
      <div className="vj-knobs" role="group">
        {Array.from({ length: KNOBS }, (_, i) => (
          <Knob key={i} index={i} knob={grid.dashboard?.[i] ?? EMPTY} picked={i === picked} onPick={() => { setPicked(i); if (linking !== null) linkKnob(i); }} />
        ))}
      </div>
      <div className="vj-dash-head">
        <input className="vj-input" aria-label={t("vj.dashboard.name")} placeholder={t("vj.dashboard.knob", { n: picked + 1 })} maxLength={40}
          value={draft ?? knob.name ?? ""} onChange={(event) => setDraft(event.target.value)} onBlur={rename}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
            if (event.key === "Escape") { setDraft(null); event.currentTarget.blur(); }
          }} />
        <button type="button" className={`vj-tb${linking === picked ? " on" : ""}`} aria-pressed={linking === picked}
          title={t("vj.dashboard.linkNote")} onClick={() => linkKnob(linking === picked ? null : picked)}>
          {t(linking === picked ? "vj.dashboard.linking" : "vj.dashboard.link")}
        </button>
      </div>
      {links.length === 0 ? (
        <p className="vj-dim vj-fx-empty">{t("vj.dashboard.noLinks")}</p>
      ) : (
        <ul className="vj-links">
          {links.map((link) => <LinkRow key={targetKey(link.target)} grid={grid} knob={picked} link={link} />)}
        </ul>
      )}
    </section>
  );
}
