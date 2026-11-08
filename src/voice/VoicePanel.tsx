import { TrackRouteField } from "../audio/routing";
import { RecoveredAudio } from "./RecoveredAudio";
import { openShowCheck } from "./ShowCheckDialog";
import { VoiceSelector } from "./VoiceSelector";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { sayError } from "../state/ui";
import { useT } from "../i18n";
import { Field, SelectField, TextAreaField, TextField, TimecodeField } from "../components/fields";
import { usePreferences } from "../preferences/store";
import { useDoc } from "../state/store";
import { useTransport, selectMode } from "../transport/store";
import type { Devices } from "../devices/useDevices";
import type { Timeline, Track, VoiceOrder } from "../project/types";
import { AudioRoute } from "../setup/pages";
import { addOrder, generateOrder, operatorEdit, useVoice, voiceError, importRecording, stopListening } from "./store";

export function VoicePage({ devices }: { devices: Devices }) {
  const t = useT();
  const voice = usePreferences((p) => p.saved.voice);
  const route = usePreferences((p) => p.saved.operator);
  const routeLost = useTransport((s) => s.position?.operatorLost || s.info?.operator?.ok === false);
  const mode = useTransport(selectMode);
  const [cacheSize, setCacheSize] = useState("0.0");
  const [testing, setTesting] = useState(false);
  useEffect(() => { void invoke<string>("voice_cache", { clear: false }).then(setCacheSize).catch(() => undefined); }, []);
  useEffect(() => () => stopListening(), []);
  const [apiKey, setApiKey] = useState("");
  const [sessionOnly, setSessionOnly] = useState(false);
  const [status, setStatus] = useState<{ configured: boolean; sessionOnly: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { let alive = true; void invoke<typeof status>("voice_key_status").then((s) => { if (alive) setStatus(s); }).catch((e) => { if (alive) sayError(voiceError(e)); }); return () => { alive = false; }; }, []);
  const keyAction = async (remove: boolean) => {
    setBusy(true);
    try {
      await invoke(remove ? "voice_key_delete" : "voice_key_set", remove ? {} : { apiKey, sessionOnly });
      setApiKey("");
      setStatus(await invoke("voice_key_status"));
      useVoice.setState({ voices: [] });
    } catch (error) { sayError(voiceError(error)); }
    finally { setBusy(false); }
  };
  return <div className="setup-fields">
    <p className="field-note">{t("voice.billing")}</p>
    <p role="status">{t(status?.configured ? status.sessionOnly ? "voice.keySession" : "voice.keySaved" : "voice.keyMissing")}</p>
    <Field label={t("voice.apiKey")}>
      {({ id }) => <input id={id} type="password" autoComplete="off" spellCheck={false} value={apiKey} onChange={(e) => setApiKey(e.target.value)} />}
    </Field>
    <label className="check-row"><input type="checkbox" checked={sessionOnly} onChange={(e) => setSessionOnly(e.target.checked)} /><span>{t("voice.sessionOnly")}</span></label>
    <div className="voice-actions">
      <button className="button" type="button" disabled={busy || !apiKey} onClick={() => void keyAction(false)}>{t("voice.saveKey")}</button>
      <button className="button" type="button" disabled={busy || !status?.configured} onClick={() => void keyAction(true)}>{t("voice.deleteKey")}</button>
    </div>
    <VoiceSelector scope="flux" value={voice} />
    {(!route || routeLost) && <p className="field-note" role="status">{t("voice.routeMissing")}</p>}
    <button type="button" className="button" onClick={openShowCheck}>{t("check.title")}</button>
    <AudioRoute kind="operator" deviceLabel="voice.device" channelsLabel="voice.channels" devices={devices} />
    <p className="field-note">{t("voice.testNote")}</p>
    <div className="voice-actions"><button type="button" className="button" disabled={mode !== "stopped" || !route || testing} onClick={() => { setTesting(true); void invoke("voice_test_output").catch((e) => sayError(voiceError(e))).finally(() => setTesting(false)); }}>{t("voice.testOutput")}</button><button type="button" className="button" onClick={stopListening}>{t("voice.stopListen")}</button></div>
    <p className="field-note">{t("voice.cacheSize", { size: cacheSize })}</p>
    <p className="field-note">{t("voice.cacheNote")}</p>
    <button type="button" className="button" onClick={() => { void invoke<string>("voice_cache", { clear: true }).then(setCacheSize).catch((e) => sayError(voiceError(e))); }}>{t("voice.clearCache")}</button>
  </div>;
}

export function OrderPanel({ track, order, timeline }: { track: Track; order: VoiceOrder; timeline: Timeline }) {
  const t = useT();
  const view = useVoice((v) => v.views[order.id]);
  const busy = useVoice((v) => v.busy);
  const mode = useTransport(selectMode);
  const voices = useVoice((v) => v.voices);
  useEffect(() => () => { void invoke("voice_preview_stop").catch(() => undefined); }, [order.id]);
  const update = (patch: Partial<Pick<VoiceOrder, "dialogue" | "alignment" | "at">>) => operatorEdit({ op: "editOrder", id: order.id, ...patch });
  return <div className="panel-body">
    <h3>{t("voice.order")}</h3>
    <p>{t(`voice.status.${view?.status ?? "ungenerated"}`)}</p>
    {busy === order.id && <p role="status">{t("voice.preparingRecording")}</p>}
    {view?.beforeOrigin && <p className="error" role="alert">{t("voice.beforeOrigin")}</p>}
    <TextAreaField key={order.id} label={t("voice.dialogue")} value={order.dialogue} onCommit={(dialogue) => void update({ dialogue })} />
    <details><summary>{t("voice.pronunciation")}</summary><p className="field-note">{t("voice.pronunciationNote")}</p><TextAreaField key={`${order.id}-pronunciation`} label={t("voice.synthesisText")} value={order.synthesisText ?? ""} onCommit={(text) => void operatorEdit({ op: "setPronunciation", id: order.id, text })} /></details>
    <TimecodeField label={t("voice.position")} frame={timeline.timecode.start + order.at} rate={timeline.timecode.frameRate} onCommit={(frame) => void update({ at: frame - timeline.timecode.start })} />
    <SelectField label={t("voice.alignment")} value={order.alignment} onChange={(alignment) => void update({ alignment: alignment as VoiceOrder["alignment"] })}>
      <option value="start">{t("voice.start")}</option><option value="end">{t("voice.end")}</option>
    </SelectField>
    <TrackRouteField track={track} />
    <VoiceSelector scope="track" trackId={track.id} value={track.kind.type === "operator" ? track.kind.voice : undefined} />
    <p className="field-note">{t("voice.effective", { voice: voices.find((v) => v.voice_id === view?.voice)?.name ?? view?.voice ?? t("voice.none"), source: t(`voice.source.${view?.source ?? "flux"}`) })}</p>
    {order.generated && <details><summary>{t("voice.generatedDialogue")}</summary><p className="voice-dialogue">{order.generated.dialogue}</p><p className="field-note">{order.generated.origin === "imported" ? t("voice.imported") : order.generated.voice}</p>{order.generated.synthesisText && <><p className="field-note">{t("voice.synthesisText")}</p><p className="voice-dialogue">{order.generated.synthesisText}</p></>}</details>}
    {order.generated && <details><summary>{t("voice.trim")}</summary><p className="field-note">{t("voice.trimNote")}</p><TextField key={`${order.id}-trim-start`} label={t("voice.trimStart")} value={view?.trimStartMs ?? "0"} inputMode="numeric" monospace onCommit={(startMs) => void operatorEdit({ op: "setTrim", id: order.id, startMs })} /><TextField key={`${order.id}-trim-end`} label={t("voice.trimEnd")} value={view?.trimEndMs ?? "0"} inputMode="numeric" monospace onCommit={(endMs) => void operatorEdit({ op: "setTrim", id: order.id, endMs })} /></details>}
    {order.generated?.origin === "imported" && <p className="field-note">{t("voice.imported")}</p>}
    <p className="field-note">{t("voice.replaceNote")}</p>
    <button className="button" type="button" disabled={busy !== null || mode !== "stopped"} onClick={() => void importRecording(order.id)}>{t("voice.importRecording")}</button>
    <button className="button" type="button" disabled={busy !== null || mode !== "stopped" || !view?.voice} onClick={() => void generateOrder(order.id)}>{t(order.generated ? "voice.regenerate" : "voice.generate")}</button>
    {mode !== "stopped" && <p className="field-note">{t("voice.stopToGenerate")}</p>}
    <button className="button" type="button" disabled={!order.generated || mode !== "stopped"} onClick={() => { void invoke("voice_preview", { project: useDoc.getState().project, id: order.id }).catch((e) => sayError(voiceError(e))); }}>{t("voice.listen")}</button>
    <button className="button" type="button" disabled={!order.generated || mode !== "stopped"} onClick={() => { void invoke("voice_preview_stop").catch((e) => sayError(voiceError(e))); }}>{t("voice.stopListen")}</button>
    <RecoveredAudio orderId={order.id} />
    <button className="button" type="button" onClick={() => void operatorEdit({ op: "duplicateOrder", id: order.id })}>{t("voice.duplicate")}</button>
    <button className="button" type="button" onClick={() => void addOrder(track.id, order.at)}>{t("voice.addOrder")}</button>
    <button className="button danger" type="button" onClick={() => void operatorEdit({ op: "removeOrder", id: order.id })}>{t("inspector.delete")}</button>
  </div>;
}
