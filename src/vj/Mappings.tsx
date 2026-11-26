// The mappings of the show as a list: what drives what, the range of a level, the way a button acts, and the
// way out. The backend owns the mappings and checks every change; this shows them and forwards the edit.

import { useState } from "react";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { TrashIcon } from "../components/icons";
import type { VideoAsset, VjButtonMode, VjFollow, VjGrid, VjMapping } from "../project/types";
import { useDoc } from "../state/store";
import { Scrub, Segments } from "./controls";
import { useEffectCatalog } from "./effects";
import { setMapMode, useMap } from "./mapping";
import { exportMappings, importMappings } from "./mapFile";
import { hasSlot, sourceKind, sourceLabel, targetClass, targetLabel, targetUnits } from "./mapModel";
import type { SourceKind } from "./mapModel";
import { acceleratorOf } from "./shortcuts";
import { edit } from "./store";

const NO_VIDEOS: VideoAsset[] = [];
const MODES: VjButtonMode[] = ["trigger", "toggle", "hold"];
const ACTION_MODES: VjButtonMode[] = ["trigger", "hold"];
const FOLLOWS: VjFollow[] = ["position", "id"];
const FILTERS: ("all" | SourceKind)[] = ["all", "midi", "osc", "dmx", "key"];

function Row({ grid, mapping }: { grid: VjGrid; mapping: VjMapping }) {
  const t = useT();
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const catalog = useEffectCatalog((state) => state.catalog);
  const kind = targetClass(mapping.target);
  const mode = mapping.mode ?? "trigger";
  const invert = mapping.invert === true;
  const set = (changed: Partial<Pick<VjMapping, "min" | "max" | "mode">> & { invert?: boolean }) =>
    void edit({ op: "setMapping", mapping: mapping.id, min: mapping.min, max: mapping.max, invert, mode, ...changed });
  const units = kind === "continuous" ? targetUnits(mapping.target, grid, catalog) : null;
  const name = targetLabel(t, mapping.target, grid, videos);
  const scrub = (tag: string, label: string, value: number, key: "min" | "max") =>
    units && (
      <Scrub label={tag} name={`${name}: ${label}`} value={value} shown={units.shown} reset={key === "min" ? units.min : units.max}
        unit={units.unit} decimals={units.decimals} perPixel={(units.max - units.min) / 160}
        step={(units.max - units.min) / 100} onMove={() => undefined} onSet={(v) => set({ [key]: Math.min(units.max, Math.max(units.min, v)) })} />
    );
  const follow = mapping.follow ?? "id";
  const dmx = mapping.source.type === "artnet" ? mapping.source : null;
  return (
    <li className={`vj-mp-row${mapping.parked ? " parked" : ""}`}>
      <span className="vj-mp-src vj-mono" title={sourceLabel(mapping.source)}>{sourceLabel(mapping.source)}</span>
      <span className="vj-mp-tgt" title={mapping.parked ? t("vj.mappings.parkedNote") : name}>
        {mapping.parked ? t("vj.mappings.parked") : name}
      </span>
      <span className="vj-mp-opt">
        {kind === "continuous" ? (
          <>
            {scrub("↓", t("vj.mappings.min"), mapping.min, "min")}
            {scrub("↑", t("vj.mappings.max"), mapping.max, "max")}
            <button type="button" className={`vj-mp-inv${invert ? " on" : ""}`} aria-pressed={invert} title={t("vj.mappings.invertNote")}
              onClick={() => set({ invert: !invert })}>{t("vj.mappings.invert")}</button>
          </>
        ) : (
          <Segments options={kind === "switch" ? MODES : ACTION_MODES} value={mode} label={(m) => t(`vj.mappings.mode.${m}` as MessageKey)}
            title={(m) => t(`vj.mappings.modeNote.${m}` as MessageKey)} onPick={(m) => set({ mode: m })} />
        )}
        {dmx && (
          <label className="vj-mp-fine" title={t("vj.mappings.fineNote")}>
            <span>{t("vj.mappings.fine")}</span>
            <input type="text" inputMode="numeric" size={3} defaultValue={dmx.fine ?? ""} key={dmx.fine ?? "none"}
              aria-label={t("vj.mappings.fineNote")} placeholder="–"
              onBlur={(event) => {
                const text = event.currentTarget.value.trim();
                const fine = text === "" ? null : Number(text);
                if (fine !== (dmx.fine ?? null)) void edit({ op: "setMappingFine", mapping: mapping.id, fine }).then((done) => { if (!done) event.currentTarget.value = String(dmx.fine ?? ""); });
              }}
              onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} />
          </label>
        )}
        {hasSlot(mapping.target) && (
          <Segments options={FOLLOWS} value={follow} label={(f) => t(`vj.mappings.follow.${f}` as MessageKey)}
            title={(f) => t(`vj.mappings.followNote.${f}` as MessageKey)} onPick={(f) => void edit({ op: "setMappingFollow", mapping: mapping.id, follow: f })} />
        )}
      </span>
      <button type="button" className="vj-mp-del" title={t("vj.mappings.delete")} aria-label={`${t("vj.mappings.delete")}: ${name}`}
        onClick={() => void edit({ op: "removeMapping", mapping: mapping.id })}><TrashIcon /></button>
    </li>
  );
}

export function MappingsPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const on = useMap((state) => state.on);
  const all = grid.mappings ?? [];
  const [filter, setFilter] = useState<"all" | SourceKind>("all");
  const mappings = filter === "all" ? all : all.filter((mapping) => sourceKind(mapping.source) === filter);
  return (
    <div className="vj-mp">
      <div className="vj-mp-bar">
        <span className="vj-dim">{t("vj.mappings.count", { n: all.length })}</span>
        <button type="button" className={`vj-tb${on ? " on" : ""}`} aria-pressed={on} onClick={() => setMapMode(!on)}>
          {t("vj.map.mode")}<kbd>{acceleratorOf("mapMidi")}</kbd>
        </button>
      </div>
      <div className="vj-mp-bar vj-mp-tools">
        <Segments options={FILTERS} value={filter} label={(f) => t(`vj.mappings.filter.${f}` as MessageKey)} onPick={setFilter} />
        <span className="vj-mp-files">
          <button type="button" className="vj-tb" title={t("vj.mapfile.importNote")} onClick={() => void importMappings()}>{t("vj.mapfile.import")}</button>
          <button type="button" className="vj-tb" title={t("vj.mapfile.exportNote")} disabled={all.length === 0 && (grid.dashboard ?? []).every((knob) => (knob.links ?? []).length === 0)}
            onClick={() => void exportMappings()}>{t("vj.mapfile.export")}</button>
        </span>
      </div>
      {mappings.length === 0 ? (
        <p className="vj-empty">{t(all.length === 0 ? "vj.mappings.empty" : "vj.mappings.emptyFilter")}</p>
      ) : (
        <ul className="vj-mp-list">
          <li className="vj-mp-row head" aria-hidden="true">
            <span>{t("vj.mappings.source")}</span><span>{t("vj.mappings.target")}</span>
          </li>
          {mappings.map((mapping) => <Row key={mapping.id} grid={grid} mapping={mapping} />)}
        </ul>
      )}
    </div>
  );
}
