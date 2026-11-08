import { useState } from "react";
import { useT } from "../i18n";
import { useDoc } from "../state/store";
import { useShow } from "./store";
import { closeShow, prepareShow, showAction } from "./api";

export function ShowRun() {
  const t = useT();
  const setlists = useDoc((d) => d.project.setlists);
  const timelines = useDoc((d) => d.project.timelines);
  const {state, preparing} = useShow();
  const [selected, setSelected] = useState("");
  const setlist = setlists?.find((s) => s.id === (state?.setlistId ?? selected)) ?? setlists?.[0];
  if (!state) return <div className="show-prepare">
    <label>{t("show.setlist")}<select value={setlist?.id ?? ""} disabled={preparing} onChange={(e) => setSelected(e.target.value)}>
      {!setlist && <option value="">{t("show.empty")}</option>}{setlists?.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
    </select></label>
    <button type="button" className="button" disabled={!setlist || preparing} onClick={() => setlist && void prepareShow(setlist.id)}>{t(preparing ? "show.preparing" : "show.prepare")}</button>
    {!setlist && <p className="field-note">{t("show.createInEdit")}</p>}
  </div>;
  return <section className="show-run" aria-label={t("show.setlist")} data-status={state.tick.status}>
    <div className="show-run-title"><span className="legend">{setlist?.name}</span><strong>{state.name}</strong><span role="status">{t(`show.status.${state.tick.status}`)}</span></div>
    <div className="show-run-next"><span>{t("show.duration")}: {state.durationText ?? t("show.indeterminate")}</span><span>{t("show.next")}</span><strong>{state.next ?? t("show.endOfShow")}</strong>
      {state.tick.turn > 0 && <span>{t("show.turn", {number:state.tick.turn})}</span>}
      {state.tick.devampPending && <span>{t("show.devampPending")}</span>}
    </div>
    {state.notes && <p className="show-run-notes">{state.notes}</p>}
    <div className="show-run-actions">
      <button type="button" className="button primary show-go" disabled={state.tick.go === "none" || state.position.deviceLost} onClick={() => void showAction({type:"go"})}>{t(`show.go.${state.tick.go}`)}</button>
      {state.tick.devampPending && <button type="button" className="button" onClick={() => void showAction({type:"cancelDevamp"})}>{t("show.cancelDevamp")}</button>}
      <select aria-label={t("show.prepareEntry")} value={state.entryId} disabled={state.position.state !== "stopped"} onChange={(e) => void showAction({type:"prepare",entryId:e.target.value})}>{setlist?.entries.map((e,index) => <option key={e.id} value={e.id} disabled={e.omitted}>{e.name || timelines.find((tl) => tl.id === e.timelineId)?.name || t("show.entryNumber", {number:index+1})}</option>)}</select>
      <button type="button" className="button" disabled={state.position.state !== "stopped"} onClick={() => void closeShow()}>{t("show.leave")}</button>
    </div>
  </section>;
}
