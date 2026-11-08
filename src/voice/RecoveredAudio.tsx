import { useI18n, useT } from "../i18n";
import { recoverJob, retrySave, useVoice, useRecording, listenJob, stopListening } from "./store";
import { selectMode, useTransport } from "../transport/store";

export function RecoveredAudio({ orderId }: { orderId?: string }) {
  const t = useT();
  const { locale } = useI18n();
  const dateFormat = new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" });
  const jobs = useVoice((v) => v.jobs);
  const unsaved = useVoice((v) => v.unsaved);
  const busy = useVoice((v) => v.busy);
  const mode = useTransport(selectMode);
  const takeId = useRecording((s) => s.take?.jobId);
  const visible = jobs.filter((j) => j.id !== takeId && (!orderId || j.orderId === orderId));
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
        <p className="field-note">{dateFormat.format(new Date(job.createdMillis))} · {t("voice.duration", { duration: job.duration })}</p>
        <p className="field-note">{job.origin === "recorded" ? t("voice.recorded") : job.origin === "imported" ? t("voice.imported") : job.voiceName ?? job.voice}{job.origin === "imported" && job.voiceName ? ` · ${job.voiceName}` : ""}</p>
        <div className="voice-actions"><button type="button" className="button" disabled={mode !== "stopped"} onClick={() => listenJob(job.id)}>{t("voice.listen")}</button><button type="button" className="button" onClick={stopListening}>{t("voice.stopListen")}</button></div>
        <p className="field-note">{job.matching ? t(job.origin !== "generated" ? "voice.recoveredImportedMatches" : "voice.recoveredMatches") : t("voice.recoveredDifferent")}</p>
        <button type="button" className="button" disabled={busy !== null || mode !== "stopped" || !job.canAttach} onClick={() => void recoverJob(job.id)}>{t("voice.useRecovered")}</button>
        {!job.canAttach && <p className="field-note">{t("voice.orderRemoved")}</p>}
      </li>)}
    </ul>
  </section>;
}
