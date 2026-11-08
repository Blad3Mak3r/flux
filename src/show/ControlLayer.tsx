import { useT } from "../i18n";
import type { TimelineControl } from "./types";

export function ControlLayer({controls, pxPerFrame}: {controls?: TimelineControl[]; pxPerFrame: number}) {
  const t = useT();
  return <div className="show-control-layer" aria-label={t("show.controlLayer")}>
    {controls?.map((c) => <div key={c.id} className="show-control-marker" style={{left:c.at*pxPerFrame, width:c.type === "loop" ? (c.end-c.at)*pxPerFrame : undefined}} data-kind={c.type}>
      <span>{t(c.type === "loop" ? "show.loop" : "show.wait")}: {c.name}</span>
    </div>)}
  </div>;
}
