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
import { HoldButton } from "./HoldButton";
import { setShowLock } from "./lock";
import { useShowLock } from "./lockStore";
import { panic } from "./panic";
import { TimelineStrip } from "../components/TimelineStrip";

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
  const locked = useShowLock((lock) => lock.locked);

  return (
    <section className="live" data-playing={playing} data-prepared={prepared} data-locked={locked} aria-labelledby="live-legend">
      <div className="screen-bar">
        <h2 id="live-legend" className="legend">
          {t("live.legend")}
        </h2>
        {prepared ? <span className="live-setlist-name" title={setlistName}>{setlistName}</span> : <TimelineStrip />}
        <button type="button" className="button" disabled={playing} onClick={openShowCheck}>{t("check.title")}</button>
        <ShowControls />
      </div>
      {prepared ? <PreparedBody /> : <PlainBody />}
    </section>
  );
}

/**
 * The lock and the panic of the show. Locked, the backend refuses to change, open or close the
 * project, and the window cannot leave Live; unlocking and panic both need a press and hold, so
 * neither happens by accident.
 */
function ShowControls() {
  const t = useT();
  const locked = useShowLock((lock) => lock.locked);
  return (
    <div className="show-controls">
      {locked ? (
        <HoldButton className="button show-lock" title={t("live.lock.unlockHint")} onHold={() => void setShowLock(false)}>
          <LockIcon locked />
          {t("live.lock.locked")}
        </HoldButton>
      ) : (
        <button type="button" className="button show-lock" title={t("live.lock.lockHint")} onClick={() => void setShowLock(true)}>
          <LockIcon locked={false} />
          {t("live.lock.lock")}
        </button>
      )}
      <HoldButton className="button panic" title={t("panic.hint")} onHold={() => void panic(t)}>
        {t("panic.label")}
      </HoldButton>
    </div>
  );
}

function LockIcon({ locked }: { locked: boolean }) {
  return (
    <svg className="icon" width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" aria-hidden="true">
      <rect x="3" y="7" width="10" height="7" rx="1" />
      <path d={locked ? "M5 7V5a3 3 0 0 1 6 0v2" : "M5 7V5a3 3 0 0 1 5.6-1.5"} />
    </svg>
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
  // Quiet by default: a panel appears only while it has something to say.
  const hasNotes = useLive((live) => (live.state?.notes.length ?? 0) > 0);
  const hasRunning = useLive((live) => (live.state?.running.length ?? 0) > 0);

  return (
    <div className="live-show">
      <section className="live-ops" aria-label={t("live.operation")}>
        <ShowHeader />
        <div className="live-ops-scroll">
          <AudioRoutingStatus problemsOnly />
          {hasCues && <NextCue />}
          {hasRunning && <Running />}
          {hasNotes && <Notes />}
          <EntryNotes />
          <RunningOrder />
          <ShowOptions />
        </div>
      </section>
      <CueList layout="sidebar" />
    </div>
  );
}
