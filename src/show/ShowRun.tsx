import { useT } from "../i18n";
import { ListIcon, PlayIcon } from "../components/icons";
import { useDoc } from "../state/store";
import { openSetlists, useUi } from "../state/ui";
import { useShow } from "./store";
import { closeShow, prepareShow, showAction } from "./api";

/** Choose a setlist and prepare it. Live keeps its ordinary layout until a show is prepared. */
export function ShowPrepare() {
  const t = useT();
  const setlists = useDoc((d) => d.project.setlists);
  const state = useShow((s) => s.state);
  const preparing = useShow((s) => s.preparing);
  const selected = useUi((s) => s.setlistSelection);
  const setSelected = (setlistSelection: string) => useUi.setState({ setlistSelection });
  const setlist = setlists?.find((s) => s.id === (state?.setlistId ?? selected)) ?? setlists?.[0];
  return <section className="show-prepare" aria-labelledby="prepare-show-title">
    <div className="show-prepare-copy"><h3 id="prepare-show-title">{t("show.setlist")}</h3><p>{t(setlist ? "show.liveHelp" : "show.createInEdit")}</p></div>
    <div className="show-prepare-actions">
      {setlist && <><select aria-label={t("show.setlist")} value={setlist.id} disabled={preparing} onChange={(e) => setSelected(e.target.value)}>{setlists?.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}</select>
        <button type="button" className="button primary" disabled={preparing} onClick={() => void prepareShow(setlist.id)}>{t(preparing ? "show.preparing" : "show.startShow")}</button></>}
      <button type="button" className="button" disabled={preparing} onClick={openSetlists}><ListIcon />{t("show.manage")}</button>
    </div>
  </section>;
}

/**
 * The fixed head of the prepared show: the current entry, its state, GO and a compact look at what
 * follows. It never scrolls, so GO stays where the operator's hand expects it.
 */
export function ShowHeader() {
  const t = useT();
  const state = useShow((s) => s.state);
  if (!state) return null;
  const next = state.next ?? t("show.endOfShow");
  const duration = state.durationText ?? t("show.indeterminate");
  return <section className="show-header" aria-label={t("show.liveCurrent")} data-status={state.tick.status}>
    <div className="show-header-main">
      <div className="show-current-entry">
        <h3 title={state.name}>{state.name}</h3>
        <div className="show-run-status" role="status">
          <span>{t(`show.status.${state.tick.status}`)}</span>
          {state.tick.turn > 0 && <span>{t("show.turn", { number: state.tick.turn })}</span>}
          {state.tick.devampPending && <strong>{t("show.devampPending")}</strong>}
        </div>
      </div>
      <div className="show-go-area">
        <button type="button" className="button primary show-go" disabled={state.tick.go === "none" || state.position.deviceLost} onClick={() => void showAction({ type: "go" })}>
          <PlayIcon /><span className="show-go-label">{t(`show.go.${state.tick.go}`)}</span><kbd>{t("show.goShortcut")}</kbd>
        </button>
        {state.tick.devampPending && <button type="button" className="button" onClick={() => void showAction({ type: "cancelDevamp" })}>{t("show.cancelDevamp")}</button>}
      </div>
    </div>
    <dl className="show-header-context">
      <div className="show-header-next"><dt>{t("show.next")}</dt><dd title={next}>{next}</dd></div>
      <div><dt>{t("show.duration")}</dt><dd>{duration}</dd></div>
    </dl>
  </section>;
}

/** The operator notes written for the current entry. Long text scrolls inside its own box. */
export function EntryNotes() {
  const t = useT();
  const notes = useShow((s) => s.state?.notes);
  if (!notes) return null;
  return <section className="show-entry-notes" aria-labelledby="show-entry-notes-legend">
    <h2 id="show-entry-notes-legend" className="legend">{t("show.notes")}</h2>
    <p className="show-run-notes">{notes}</p>
  </section>;
}

/** Leaving the show is rare and needs the transport stopped, so it sits in a disclosure. */
export function ShowOptions() {
  const t = useT();
  const stopped = useShow((s) => s.state?.position.state === "stopped");
  return <details className="show-run-options">
    <summary>{t("show.showOptions")}</summary>
    <div className="show-run-option-fields">
      <button type="button" className="button" disabled={!stopped} onClick={() => void closeShow()}>{t("show.leave")}</button>
      {!stopped && <p>{t("show.stopFirst")}</p>}
    </div>
  </details>;
}
