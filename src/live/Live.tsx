import { ShowPrepare, ShowHeader, EntryNotes, ShowOptions } from "../show/ShowRun";
import { RunningOrder } from "../show/RunningOrder";
import { useShow } from "../show/store";
import { AudioRoutingStatus } from "../audio/routing";
import { openShowCheck } from "../voice/ShowCheckDialog";
import { useT } from "../i18n";
import { setView } from "../state/ui";
import { selectMode, useTransport } from "../transport/store";
import { CueList } from "./CueList";
import { NextCue } from "./NextCue";
import { Notes } from "./Notes";
import { Running } from "./Running";
import { useLive } from "./store";
import { TimelineStrip } from "../components/TimelineStrip";
import { ViewSwitch } from "./ViewSwitch";

/**
 * The timeline as it is run on stage. Everything on it comes from the backend (cues, their state,
 * the countdowns) and this only draws it; the transport and the timecode stay on the faceplate.
 *
 * With a prepared show the body has two columns: the operation of the show on the left (a fixed
 * head with GO, then a scrolling column) and the cues on the right, always in view.
 */
export function Live() {
  const t = useT();
  const setlistName = useShow((s) => s.state?.setlistName);
  const prepared = setlistName !== undefined;
  const playing = useTransport((transport) => selectMode(transport) === "playing");

  return (
    <section className="live" data-playing={playing} data-prepared={prepared} aria-labelledby="live-legend">
      <div className="screen-bar">
        <h2 id="live-legend" className="legend">
          {t("live.legend")}
        </h2>
        {prepared ? <span className="live-setlist-name" title={setlistName}>{setlistName}</span> : <TimelineStrip />}
        <button type="button" className="button" disabled={playing} onClick={openShowCheck}>{t("check.title")}</button>
        <ViewSwitch />
      </div>
      {prepared ? <PreparedBody /> : <PlainBody />}
    </section>
  );
}

/** Live without a setlist: the show surface is the timeline's own cues, stacked. */
function PlainBody() {
  const t = useT();
  const ready = useLive((live) => live.ready);
  const empty = useLive((live) => live.cues.length === 0);
  const hasNotes = useLive((live) => live.cues.some((cue) => cue.kind === "note"));

  return (
    <>
      <ShowPrepare />
      <AudioRoutingStatus />
      {empty && !ready ? (
        <div className="empty-state">
          <p className="empty-title">{t("live.loading")}</p>
        </div>
      ) : empty ? (
        <div className="empty-state">
          <p className="empty-title">{t("live.empty.title")}</p>
          <p>{t("live.empty.body")}</p>
          <div className="button-row">
            <button type="button" className="button" onClick={() => setView("edit")}>
              {t("live.empty.action")}
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="live-head">
            <div className="live-top">
              <NextCue />
              <Running />
            </div>
            {hasNotes && <Notes />}
          </div>
          <CueList />
        </>
      )}
    </>
  );
}

/** A prepared show: operation on the left, the cue sidebar on the right. */
function PreparedBody() {
  const t = useT();
  const hasCues = useLive((live) => live.cues.length > 0);
  const hasNotes = useLive((live) => live.cues.some((cue) => cue.kind === "note"));

  return (
    <div className="live-show">
      <section className="live-ops" aria-label={t("live.operation")}>
        <ShowHeader />
        <div className="live-ops-scroll">
          <AudioRoutingStatus />
          {hasCues && (
            <>
              <NextCue />
              <Running />
              {hasNotes && <Notes />}
            </>
          )}
          <EntryNotes />
          <RunningOrder />
          <ShowOptions />
        </div>
      </section>
      <CueList layout="sidebar" />
    </div>
  );
}
