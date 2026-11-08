import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Field } from "../components/fields";
import { listAudioHosts, type AudioHost } from "../devices/api";
import { useT } from "../i18n";
import { sayError } from "../state/ui";
import { changePreferences, usePreferences } from "../preferences/store";
import { selectMode, useTransport } from "../transport/store";
import { acceptTake, discardTake, leaveRecording, listenJob, startRecording, stopListening, stopRecording, useRecording, useVoice, voiceError } from "./store";

interface Input { name: string; channels: number[] }
interface CaptureStatus { duration: string; peak: number; done: boolean; error: string | null }

export function RecordingPanel({ orderId }: { orderId: string }) {
  const t = useT();
  const mic = usePreferences((s) => s.saved.microphone);
  const preferencesError = usePreferences((s) => s.error);
  const active = useRecording((s) => s.active);
  const take = useRecording((s) => s.take);
  const working = useRecording((s) => s.working);
  const busy = useVoice((s) => s.busy);
  const mode = useTransport(selectMode);
  const [hosts, setHosts] = useState<AudioHost[]>([]);
  const [inputs, setInputs] = useState<Input[]>([]);
  const [loading, setLoading] = useState(true);
  const [status, setStatus] = useState<CaptureStatus | null>(null);
  const [refresh, setRefresh] = useState(0);
  const driver = mic?.driver ?? null;
  const selected = inputs.find((i) => i.name === mic?.device);
  useEffect(() => {
    let disposed = false;
    setLoading(true);
    void Promise.all([listAudioHosts(), invoke<Input[]>("voice_inputs", { driver })])
      .then(([hosts, inputs]) => { if (!disposed) { setHosts(hosts); setInputs(inputs); } })
      .catch((error) => { if (!disposed) { setInputs([]); sayError(voiceError(error)); } })
      .finally(() => { if (!disposed) setLoading(false); });
    return () => { disposed = true; };
  }, [driver, refresh]);
  useEffect(() => () => { void leaveRecording(); }, [orderId]);
  useEffect(() => {
    if (active !== orderId) { setStatus(null); return; }
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await invoke<CaptureStatus | null>("voice_record_status");
        if (disposed) return;
        setStatus(next);
        if (next?.done) { await stopRecording(); return; }
      } catch (error) { if (!disposed) sayError(voiceError(error)); }
      if (!disposed) timer = setTimeout(() => { void poll(); }, 100);
    };
    void poll();
    return () => { disposed = true; clearTimeout(timer); };
  }, [active, orderId]);
  const locked = active !== null || working;
  const choose = (device: string, channel: number, host = driver) => {
    void changePreferences({ op: "microphone", microphone: { driver: host, device, channel } }).catch(() => undefined);
  };
  return <section className="voice-recording" aria-label={t("voice.recordMicrophone")}>
    <h4>{t("voice.recordMicrophone")}</h4>
    <Field label={t("voice.inputDriver")}>{({ id }) => <select id={id} value={driver ?? ""} disabled={locked} onChange={(e) => choose(mic?.device ?? "", 1, e.target.value || null)}>
      <option value="">{t("voice.systemDriver")}</option>
      {hosts.map((h) => <option key={h.name} value={h.name}>{h.name}</option>)}
    </select>}</Field>
    <Field label={t("voice.inputDevice")}>{({ id }) => <select id={id} value={mic?.device ?? ""} disabled={locked || loading} onChange={(e) => choose(e.target.value, 1)}>
      <option value="" disabled>{t(loading ? "voice.loadingInputs" : "voice.chooseInput")}</option>
      {mic?.device && !selected && <option value={mic.device}>{t("voice.inputMissing", { name: mic.device })}</option>}
      {inputs.map((input) => <option key={input.name} value={input.name}>{input.name}</option>)}
    </select>}</Field>
    <Field label={t("voice.inputChannel")}>{({ id }) => <select id={id} value={mic?.channel ?? 1} disabled={locked || !selected} onChange={(e) => {
      const channel = selected?.channels.find((c) => String(c) === e.target.value);
      if (channel && mic) choose(mic.device, channel);
    }}>
      {selected?.channels.map((c) => <option key={c} value={c}>{c}</option>)}
    </select>}</Field>
    <button type="button" className="button" disabled={locked} onClick={() => setRefresh((r) => r + 1)}>{t("voice.refreshInputs")}</button>
    {preferencesError && <p className="error" role="alert">{preferencesError}</p>}
    {active === orderId && <>
      <p className="voice-record-state" role="status">{t("voice.recordingActive")} · {t("voice.duration", { duration: status?.duration ?? "0.0" })}</p>
      <Field label={t("voice.inputLevel")}>{({ id }) => <meter id={id} className="voice-input-meter" min={0} max={1} value={status?.peak ?? 0} />}</Field>
      <button type="button" className="button" disabled={working} onClick={() => void stopRecording()}>{t("voice.stopRecording")}</button>
      <button type="button" className="button" disabled={working} onClick={() => void leaveRecording()}>{t("voice.cancelTake")}</button>
    </>}
    {take?.orderId === orderId ? <>
      <p role="status">{t("voice.takeReady")}</p>
      <div className="voice-actions">
        <button type="button" className="button" disabled={working || mode !== "stopped"} onClick={() => listenJob(take.jobId)}>{t("voice.listenTake")}</button>
        <button type="button" className="button" onClick={stopListening}>{t("voice.stopListen")}</button>
        <button type="button" className="button primary" disabled={working || busy !== null || mode !== "stopped"} onClick={() => void acceptTake()}>{t("voice.useTake")}</button>
        <button type="button" className="button" disabled={working || busy !== null || mode !== "stopped"} onClick={() => { void discardTake().then(() => { if (!useRecording.getState().take) return startRecording(orderId); }); }}>{t("voice.repeatTake")}</button>
        <button type="button" className="button" disabled={working} onClick={() => void discardTake()}>{t("voice.cancelTake")}</button>
      </div>
    </> : active === null && <button type="button" className="button" disabled={working || busy !== null || mode !== "stopped" || !selected || loading} onClick={() => void startRecording(orderId)}>{t("voice.record")}</button>}
    <p className="field-note">{t("voice.recordingNote")}</p>
  </section>;
}
