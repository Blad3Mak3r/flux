// The mappings of the show as a list: what drives what, the range of a level, the way a button acts, and the
// way out. The backend owns the mappings and checks every change; this shows them and forwards the edit.

import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { TrashIcon } from "../components/icons";
import type { VideoAsset, VjButtonMode, VjGrid, VjMapping } from "../project/types";
import { useDoc } from "../state/store";
import { Scrub, Segments } from "./controls";
import { useEffectCatalog } from "./effects";
import { setMapMode, useMap } from "./mapping";
import { sourceLabel, targetClass, targetLabel, targetUnits } from "./mapModel";
import { edit } from "./store";

const NO_VIDEOS: VideoAsset[] = [];
const MODES: VjButtonMode[] = ["trigger", "toggle", "hold"];
const ACTION_MODES: VjButtonMode[] = ["trigger", "hold"];

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
  return (
    <li className="vj-mp-row">
      <span className="vj-mp-src vj-mono" title={sourceLabel(mapping.source)}>{sourceLabel(mapping.source)}</span>
      <span className="vj-mp-tgt" title={name}>{name}</span>
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
      </span>
      <button type="button" className="vj-mp-del" title={t("vj.mappings.delete")} aria-label={`${t("vj.mappings.delete")}: ${name}`}
        onClick={() => void edit({ op: "removeMapping", mapping: mapping.id })}><TrashIcon /></button>
    </li>
  );
}

export function MappingsPanel({ grid }: { grid: VjGrid }) {
  const t = useT();
  const on = useMap((state) => state.on);
  const mappings = grid.mappings ?? [];
  return (
    <div className="vj-mp">
      <div className="vj-mp-bar">
        <span className="vj-dim">{t("vj.mappings.count", { n: mappings.length })}</span>
        <button type="button" className={`vj-tb${on ? " on" : ""}`} aria-pressed={on} onClick={() => setMapMode(!on)}>
          {t("vj.map.mode")}<kbd>Ctrl+M</kbd>
        </button>
      </div>
      {mappings.length === 0 ? (
        <p className="vj-empty">{t("vj.mappings.empty")}</p>
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
