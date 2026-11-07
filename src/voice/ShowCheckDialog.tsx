import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useT } from "../i18n";
import { useDoc } from "../state/store";
import { openSetup, select, setView, useUi } from "../state/ui";
import { usePreferences } from "../preferences/store";
import { selectMode, useTransport } from "../transport/store";
import { selectTimeline } from "../state/timelines";
import { seek } from "../transport/playback";
import type { Project } from "../project/types";
import { CloseIcon } from "../components/icons";
import { voiceError, stopListening, useVoice } from "./store";
import { RecoveredAudio } from "./RecoveredAudio";

type IssueCode = "ungenerated" | "stale" | "beforeOrigin" | "missingAudio" | "overlap" | "missingRoute" | "ltcConflict" | "silentOutput";
export interface ShowIssue { code: IssueCode; blocking: boolean; timelineId: string | null; timelineName: string | null; orderId: string | null; name: string; frame: number | null }
export interface ShowReport { ready: number; orders: number; errors: number; warnings: number; issues: ShowIssue[] }
export const openShowCheck = () => useUi.setState({ showCheck: true, setup: null, menu: null });
const close = () => useUi.setState({ showCheck: false });

export function ShowCheckDialog() {
  const t = useT();
  const open = useUi((s) => s.showCheck);
  const project = useDoc((s) => s.project);
  const revision = usePreferences((s) => s.saved.revision);
  const mode = useTransport(selectMode);
  const voiceBusy = useVoice((s) => s.busy);
  const voiceProblem = useVoice((s) => s.error);
  const dialog = useRef<HTMLDialogElement>(null);
  const sequence = useRef(0);
  const [result, setResult] = useState<{ report: ShowReport; project: Project; revision: number } | null>(null);
  const [issueIndex, setIssueIndex] = useState(0);
  const moveIssue = (index: number) => { setIssueIndex(index); document.getElementById(`voice-check-issue-${index}`)?.focus(); };
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const stale = result !== null && (result.project !== project || result.revision !== revision);

  const run = async () => {
    const request = ++sequence.current;
    const project = useDoc.getState().project;
    const revision = usePreferences.getState().saved.revision;
    setBusy(true); setError(null); setIssueIndex(0);
    try {
      const report = await invoke<ShowReport>("operator_preflight", { project });
      if (request === sequence.current) setResult({ report, project, revision });
    } catch (error) { if (request === sequence.current) setError(voiceError(error)); }
    finally { if (request === sequence.current) setBusy(false); }
  };

  useEffect(() => {
    if (open) { dialog.current?.showModal(); setResult(null); void run(); }
    else { dialog.current?.close(); sequence.current++; setBusy(false); stopListening(); }
  }, [open]);

  const visit = async (issue: ShowIssue) => {
    if (issue.timelineId) await selectTimeline(issue.timelineId);
    if (useTransport.getState().position?.state === "playing") return;
    close();
    if (issue.orderId) { setView("edit"); select({ kind: "operator", id: issue.orderId }); if (issue.frame !== null) await seek(issue.frame); }
    else openSetup("operator");
  };

  return <dialog ref={dialog} className="dialog show-check" aria-labelledby="show-check-title" onCancel={close}>
    <header className="setup-head"><h2 id="show-check-title">{t("check.title")}</h2><button type="button" className="icon-button" aria-label={t("check.close")} onClick={close}><CloseIcon /></button></header>
    <div className="show-check-body">
      <p className="field-note">{t("check.note")}</p>
      <button type="button" className="button" disabled={busy || mode !== "stopped"} onClick={() => void run()}>{t(busy ? "check.running" : "check.run")}</button>
      {voiceBusy && <p role="status">{t("voice.preparingRecording")}</p>}
      {voiceProblem && <p className="error" role="alert">{voiceProblem}</p>}
      {error && <p className="error" role="alert">{error}</p>}
      {stale && <p className="warning" role="status">{t("check.changed")}</p>}
      {result && <>
        <p role="status">{t("check.summary", { ready: result.report.ready, orders: result.report.orders, errors: result.report.errors, warnings: result.report.warnings })}</p>
        {result.report.issues.length === 0 && <p>{t("check.clear")}</p>}
        {result.report.issues.length > 0 && <div className="voice-actions" aria-label={t("check.navigation")}><button type="button" className="button" disabled={issueIndex <= 0} onClick={() => moveIssue(issueIndex - 1)}>{t("check.previous")}</button><button type="button" className="button" disabled={issueIndex >= result.report.issues.length - 1} onClick={() => moveIssue(issueIndex + 1)}>{t("check.next")}</button></div>}
        <ul className="show-check-list">
          {result.report.issues.map((issue, i) => <li id={`voice-check-issue-${i}`} tabIndex={-1} aria-current={issueIndex === i ? "true" : undefined} onFocus={() => setIssueIndex(i)} key={`${issue.code}-${issue.orderId ?? "route"}-${i}`}>
            <div><strong>{t(issue.blocking ? "check.error" : "check.warning")}</strong><p>{t(`check.issue.${issue.code}`)}</p>{issue.name && <p className="voice-dialogue">{issue.name}</p>}{issue.timelineName && <p className="field-note">{issue.timelineName}</p>}</div>
            <button type="button" className="button" disabled={stale || mode !== "stopped"} onClick={() => void visit(issue)}>{t(issue.orderId ? "check.goToOrder" : "menu.setup")}</button>
          </li>)}
        </ul>
      </>}
      <RecoveredAudio />
    </div>
  </dialog>;
}
