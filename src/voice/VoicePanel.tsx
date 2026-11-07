import { VoiceSelector } from "./VoiceSelector";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useT } from "../i18n";
import { Field, SelectField, TextAreaField, TimecodeField } from "../components/fields";
import { usePreferences } from "../preferences/store";
import { useDoc } from "../state/store";
import { useTransport, selectMode } from "../transport/store";
import type { Devices } from "../devices/useDevices";
import type { Timeline, Track, VoiceOrder } from "../project/types";
import { AudioRoute } from "../setup/pages";
import { addOrder, generateOrder, operatorEdit, useVoice, voiceError } from "./store";

export function VoicePage({ devices }: { devices: Devices }) {
  const t = useT();
  const voice = usePreferences((p) => p.saved.voice);
  const error = useVoice((v) => v.error);
  const route = usePreferences((p) => p.saved.operator);
  const routeLost = useTransport((s) => s.position?.operatorLost || s.info?.operator?.ok === false);
  const [apiKey, setApiKey] = useState("");
  const [sessionOnly, setSessionOnly] = useState(false);
  const [status, setStatus] = useState<{ configured: boolean; sessionOnly: boolean } | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { let alive = true; void invoke<typeof status>("voice_key_status").then((s) => { if (alive) setStatus(s); }).catch((e) => { if (alive) setProblem(voiceError(e)); }); return () => { alive = false; }; }, []);
  const keyAction = async (remove: boolean) => {
    setBusy(true); setProblem(null);
    try {
      await invoke(remove ? "voice_key_delete" : "voice_key_set", remove ? {} : { apiKey, sessionOnly });
      setApiKey("");
      setStatus(await invoke("voice_key_status"));
      useVoice.setState({ voices: [], error: null, listError: null });
    } catch (error) { setProblem(voiceError(error)); }
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
    {problem && <p className="error" role="alert">{problem}</p>}
    <VoiceSelector scope="flux" value={voice} />
    {error && <p className="error" role="alert">{error}</p>}
    {(!route || routeLost) && <p className="field-note" role="status">{t("voice.routeMissing")}</p>}
    <AudioRoute kind="operator" deviceLabel="voice.device" channelsLabel="voice.channels" devices={devices} />
  </div>;
}

export function OrderPanel({ track, order, timeline }: { track: Track; order: VoiceOrder; timeline: Timeline }) {
  const t = useT();
  const view = useVoice((v) => v.views[order.id]);
  const busy = useVoice((v) => v.busy);
  const error = useVoice((v) => v.error);
  const mode = useTransport(selectMode);
  const voices = useVoice((v) => v.voices);
  useEffect(() => () => { void invoke("voice_preview_stop").catch(() => undefined); }, []);
  const update = (patch: Partial<Pick<VoiceOrder, "dialogue" | "alignment" | "at">>) => operatorEdit({ op: "editOrder", id: order.id, dialogue: order.dialogue, alignment: order.alignment, at: order.at, ...patch });
  return <div className="panel-body">
    <h3>{t("voice.order")}</h3>
    <p role="status">{t(busy === order.id ? "voice.generating" : `voice.status.${view?.status ?? "ungenerated"}`)}</p>
    {view?.beforeOrigin && <p className="error" role="alert">{t("voice.beforeOrigin")}</p>}
    <TextAreaField key={order.id} label={t("voice.dialogue")} value={order.dialogue} onCommit={(dialogue) => void update({ dialogue })} />
    <TimecodeField label={t("voice.position")} frame={timeline.timecode.start + order.at} rate={timeline.timecode.frameRate} onCommit={(frame) => void update({ at: frame - timeline.timecode.start })} />
    <SelectField label={t("voice.alignment")} value={order.alignment} onChange={(alignment) => void update({ alignment: alignment as VoiceOrder["alignment"] })}>
      <option value="start">{t("voice.start")}</option><option value="end">{t("voice.end")}</option>
    </SelectField>
    <VoiceSelector scope="track" trackId={track.id} value={track.kind.type === "operator" ? track.kind.voice : undefined} />
    <p className="field-note">{t("voice.effective", { voice: voices.find((v) => v.voice_id === view?.voice)?.name ?? view?.voice ?? t("voice.none"), source: t(`voice.source.${view?.source ?? "flux"}`) })}</p>
    {order.generated && <details><summary>{t("voice.generatedDialogue")}</summary><p className="voice-dialogue">{order.generated.dialogue}</p><p className="field-note">{order.generated.voice}</p></details>}
    <button className="button" type="button" disabled={busy !== null || mode !== "stopped" || !view?.voice} onClick={() => void generateOrder(order.id)}>{t(order.generated ? "voice.regenerate" : "voice.generate")}</button>
    {mode !== "stopped" && <p className="field-note">{t("voice.stopToGenerate")}</p>}
    <button className="button" type="button" disabled={!order.generated || mode === "playing"} onClick={() => { void invoke("voice_preview", { project: useDoc.getState().project, id: order.id }).catch((e) => useVoice.setState({ error: voiceError(e) })); }}>{t("voice.listen")}</button>
    {error && <p className="error" role="alert">{error}</p>}
    <button className="button" type="button" onClick={() => void operatorEdit({ op: "duplicateOrder", id: order.id })}>{t("voice.duplicate")}</button>
    <button className="button" type="button" onClick={() => void addOrder(track.id, order.at)}>{t("voice.addOrder")}</button>
    <button className="button danger" type="button" onClick={() => void operatorEdit({ op: "removeOrder", id: order.id })}>{t("inspector.delete")}</button>
  </div>;
}
