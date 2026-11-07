import { useT } from "../i18n";
import { recoverJob, retrySave, useVoice } from "./store";
import { selectMode, useTransport } from "../transport/store";

export function RecoveredAudio({ orderId }: { orderId?: string }) {
  const t = useT();
  const jobs = useVoice((v) => v.jobs);
  const unsaved = useVoice((v) => v.unsaved);
  const busy = useVoice((v) => v.busy);
  const mode = useTransport(selectMode);
  const visible = orderId ? jobs.filter((j) => j.orderId === orderId) : jobs;
  if (!unsaved && visible.length === 0) return null;
  return <section className="recovered-audio" aria-label={t("voice.recovered")}>
    <h4>{t("voice.recovered")}</h4>
    <p className="field-note">{t("voice.recoveredNote")}</p>
    {unsaved && <>
      <p className="warning" role="status">{t("voice.unsavedResult")}</p>
      <button type="button" className="button" disabled={busy !== null} onClick={() => void retrySave()}>{t("voice.retrySave")}</button>
    </>}
    <ul className="recovered-list">
      {visible.map((job) => <li key={job.id}>
        <p className="voice-dialogue">{job.dialogue}</p>
        <p className="field-note">{job.matching ? t("voice.recoveredMatches") : t("voice.recoveredDifferent")}</p>
        <button type="button" className="button" disabled={busy !== null || mode !== "stopped" || !job.canAttach} onClick={() => void recoverJob(job.id)}>{t("voice.useRecovered")}</button>
        {!job.canAttach && <p className="field-note">{t("voice.orderRemoved")}</p>}
      </li>)}
    </ul>
  </section>;
}
