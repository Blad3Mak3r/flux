import { invoke } from "@tauri-apps/api/core";
import { useT } from "../i18n";
import { translator, initialLocale, type MessageKey } from "../i18n/messages";
import type { AudioAssignment, ProjectState, Track } from "../project/types";
import { usePreferences } from "../preferences/store";
import { dispatch, useDoc } from "../state/store";
import { sayError, useUi } from "../state/ui";
import { useTransport } from "../transport/store";
import { SelectField } from "../components/fields";

export type AudioRoutingOp = { op: "add"; name: string } | { op: "rename"; id: string; name: string } | { op: "remove"; id: string } | { op: "assign"; trackId: string; output: AudioAssignment };
let pending = Promise.resolve();
export function audioRoutingEdit(op: AudioRoutingOp): Promise<boolean> {
  let applied = false;
  pending = pending.then(async () => {
    const project = useDoc.getState().project;
    const state = await invoke<ProjectState>("audio_routing_edit", { project, op });
    if (useDoc.getState().project.meta.id === project.meta.id) { dispatch({ type: "apply", ...state }); applied = true; }
  }).catch((error) => {
    const code = (error as { code?: string })?.code;
    const key = ["invalidId", "invalidName", "duplicateName", "unknownOutput", "unsupportedTrack", "unknownTrack"].includes(code ?? "") ? `routing.error.${code}` as MessageKey : "routing.error.invalidProject";
    sayError(translator(usePreferences.getState().saved.locale ?? initialLocale())(key));
  });
  return pending.then(() => applied);
}

export function useAudioOutputName() {
  const t = useT();
  const choices = usePreferences((s) => s.choices);
  return (id: string | null | undefined): string => id === "programme" ? t("routing.programme") : id === "operator" ? t("routing.operator") : id ? choices.find((c) => c.id === id)?.name ?? t("routing.missing") : t("routing.silent");
}

/** Effective targets and pending/active membership are supplied by the backend. */
export function TrackRouteField({ track }: { track: Track }) {
  const t = useT();
  const name = useAudioOutputName();
  const choices = usePreferences((s) => s.choices);
  const target = usePreferences((s) => s.output.trackRoutes?.find((r) => r.trackId === track.id)?.output);
  const routing = useTransport((s) => s.audioRouting);
  const view = useUi((s) => s.view);
  const preview = usePreferences((s) => s.audition);
  const activeTarget = routing?.activeTracks.find((r) => r.trackId === track.id)?.output;
  const health = routing?.active.find((r) => r.id === activeTarget);
  const assignment = track.audioOutput;
  const value = assignment?.type === "output" ? assignment.id : assignment?.type ?? "inherit";
  if (track.kind.type !== "audio" && track.kind.type !== "operator") return null;
  return <>
    <SelectField label={t("routing.trackOutput")} value={value} hint={t(view === "edit" && preview ? "routing.previewActive" : "routing.effective", { output: name(target) })}
      onChange={(id) => void audioRoutingEdit({ op: "assign", trackId: track.id, output: id === "inherit" || id === "silent" ? { type: id } : { type: "output", id } })}>
      <option value="inherit">{t("routing.inherit")}</option>
      <option value="silent">{t("routing.silent")}</option>
      {choices.map((c) => <option key={c.id} value={c.id}>{name(c.id)}</option>)}
    </SelectField>
    {health && health.status !== "ready" && health.status !== "unused" && <p className="field-note warning" role="status">{t(`routing.state.${health.status}`)}</p>}
    {routing?.pending && <p className="field-note" role="status">{t("routing.pending")} {t("routing.active", { output: name(routing.activeTracks.find((r) => r.trackId === track.id)?.output) })}</p>}
  </>;
}

/** `problemsOnly` hides routes that are ready, for screens where only what needs attention should speak. */
export function AudioRoutingStatus({ problemsOnly = false }: { problemsOnly?: boolean }) {
  const t = useT();
  const name = useAudioOutputName();
  const routing = useTransport((s) => s.audioRouting);
  if (!routing) return null;
  return <div className="audio-routing-status" role="status">
    {routing.pending && <p className="field-note">{t("routing.pending")}</p>}
    {routing.active.map((r) => r.status !== "unused" && !(problemsOnly && r.status === "ready") && <span key={r.id} className="row-status">
      <span className="lamp" data-state={r.status === "ready" ? "on" : "fault"} aria-hidden="true" />
      {name(r.id)} · {t(`routing.state.${r.status}`)}
    </span>)}
  </div>;
}
