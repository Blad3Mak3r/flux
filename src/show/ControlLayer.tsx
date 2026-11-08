import { useT } from "../i18n";
import type { TimelineControl } from "./types";
import { openShowControls, useUi } from "../state/ui";
import { LoopIcon, PauseIcon, PlusIcon } from "../components/icons";

export const CONTROL_RAIL_H = 28;

export function ControlLayer({controls, pxPerFrame}: {controls?: TimelineControl[]; pxPerFrame: number}) {
  const t = useT();
  const selected = useUi((s) => s.controlEditor);
  return <div className="show-control-layer" aria-label={t("show.controlLayer")}>
    {!controls?.length && <button type="button" className="control-rail-empty" onClick={() => openShowControls()}><PlusIcon />{t("show.addRailControl")}</button>}
    {controls?.map((c) => <div key={c.id} className="show-control-marker" style={{left:c.at*pxPerFrame, width:c.type === "loop" ? (c.end-c.at)*pxPerFrame : undefined}} data-kind={c.type} data-selected={selected === c.id}>
      <button type="button" aria-pressed={selected === c.id} title={`${t(c.type === "loop" ? "show.loop" : "show.wait")}: ${c.name}`} onClick={() => openShowControls(c.id)}>{c.type === "loop" ? <LoopIcon /> : <PauseIcon />}<span>{c.name}</span></button>
    </div>)}
  </div>;
}
