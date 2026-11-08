import { useT } from "../i18n";
import { ListIcon, PlayIcon } from "../components/icons";
import { useDoc } from "../state/store";
import { openSetlists, useUi } from "../state/ui";
import { useShow } from "./store";
import { closeShow, prepareShow, showAction } from "./api";

export function ShowRun() {
  const t = useT();
  const setlists = useDoc((d) => d.project.setlists);
  const timelines = useDoc((d) => d.project.timelines);
  const state = useShow((s) => s.state);
  const preparing = useShow((s) => s.preparing);
  const selected = useUi((s) => s.setlistSelection);
  const setSelected = (setlistSelection: string) => useUi.setState({setlistSelection});
  const setlist = setlists?.find((s) => s.id === (state?.setlistId ?? selected)) ?? setlists?.[0];
  if (!state) return <section className="show-prepare" aria-labelledby="prepare-show-title">
    <div className="show-prepare-copy"><h3 id="prepare-show-title">{t("show.setlist")}</h3><p>{t(setlist ? "show.liveHelp" : "show.createInEdit")}</p></div>
    <div className="show-prepare-actions">
      {setlist && <><select aria-label={t("show.setlist")} value={setlist.id} disabled={preparing} onChange={(e) => setSelected(e.target.value)}>{setlists?.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}</select>
        <button type="button" className="button primary" disabled={preparing} onClick={() => void prepareShow(setlist.id)}>{t(preparing ? "show.preparing" : "show.startShow")}</button></>}
      <button type="button" className="button" disabled={preparing} onClick={openSetlists}><ListIcon />{t("show.manage")}</button>
    </div>
  </section>;
  const stopped = state.position.state === "stopped";
  return <section className="show-run" aria-label={t("show.setlist")} data-status={state.tick.status}>
    <div className="show-run-main">
      <div className="show-current-entry"><h3>{state.name}</h3><div className="show-run-status" role="status"><span>{t(`show.status.${state.tick.status}`)}</span>{state.tick.turn > 0 && <span>{t("show.turn",{number:state.tick.turn})}</span>}{state.tick.devampPending && <strong>{t("show.devampPending")}</strong>}</div></div>
      <div className="show-go-area"><button type="button" className="button primary show-go" disabled={state.tick.go === "none" || state.position.deviceLost} onClick={() => void showAction({type:"go"})}><PlayIcon />{t(`show.go.${state.tick.go}`)}<kbd>{t("show.goShortcut")}</kbd></button>
        {state.tick.devampPending && <button type="button" className="button" onClick={() => void showAction({type:"cancelDevamp"})}>{t("show.cancelDevamp")}</button>}
      </div>
    </div>
    <div className="show-run-context"><div><span>{t("show.next")}</span><strong>{state.next ?? t("show.endOfShow")}</strong></div><div><span>{t("show.duration")}</span><strong>{state.durationText ?? t("show.indeterminate")}</strong></div><div><span>{t("show.setlist")}</span><strong>{setlist?.name}</strong></div></div>
    {state.notes && <p className="show-run-notes">{state.notes}</p>}
    <details className="show-run-options"><summary>{t("show.moreActions")}</summary><div className="show-run-option-fields"><label>{t("show.prepareEntry")}<select value={state.entryId} disabled={!stopped} onChange={(e) => void showAction({type:"prepare",entryId:e.target.value})}>{setlist?.entries.map((e,index) => <option key={e.id} value={e.id} disabled={e.omitted}>{e.name || timelines.find((tl) => tl.id === e.timelineId)?.name || t("show.entryNumber",{number:index+1})}</option>)}</select></label><button type="button" className="button" disabled={!stopped} onClick={() => void closeShow()}>{t("show.leave")}</button>{!stopped && <p>{t("show.stopFirst")}</p>}</div></details>
  </section>;
}
