import { invoke, isTauri } from "@tauri-apps/api/core";
import { create } from "zustand";
import { useDoc, dispatch } from "../state/store";
import { useUi, select, sayError } from "../state/ui";
import { usePreferences } from "../preferences/store";
import type { ProjectState, VoiceView } from "../project/types";
import { translator, initialLocale, type MessageKey } from "../i18n/messages";
import { analyse } from "../state/audio";
import { flushDocumentSync } from "../state/persist";

export interface Voice { voice_id: string; name: string }
export interface RecoverableJob { id: string; orderId: string; dialogue: string; voice: string; canAttach: boolean; matching: boolean }
export const useVoice = create<{ views: Record<string, VoiceView>; voices: Voice[]; busy: string | null; error: string | null; listError: string | null; loading: boolean; jobs: RecoverableJob[]; unsaved: boolean }>(() => ({ views: {}, voices: [], busy: null, error: null, listError: null, loading: false, jobs: [], unsaved: false }));

export function voiceError(error: unknown): string {
  const value = error as { code?: string; error?: { code?: string } };
  const code = value?.code === "provider" ? value.error?.code : value?.code;
  return translator(usePreferences.getState().saved.locale ?? initialLocale())(`voice.error.${code ?? "provider"}` as MessageKey);
}

let revision = 0;
export async function refreshVoiceViews(): Promise<void> {
  if (!isTauri()) return;
  const current = ++revision;
  try {
    const views = await invoke<VoiceView[]>("operator_views", { project: useDoc.getState().project });
    if (current === revision) useVoice.setState({ views: Object.fromEntries(views.map((v) => [v.id, v])) });
  } catch (error) { if (current === revision) useVoice.setState({ error: voiceError(error) }); }
}

let jobsRevision = 0;
export async function refreshJobs(): Promise<void> {
  if (!isTauri()) return;
  const request = ++jobsRevision;
  const project = useDoc.getState().project;
  try {
    const [jobs, unsaved] = await Promise.all([
      invoke<RecoverableJob[]>("voice_jobs", { project }),
      invoke<boolean>("voice_pending", { projectId: project.meta.id }),
    ]);
    if (request === jobsRevision) useVoice.setState({ jobs, unsaved });
  } catch (error) { if (request === jobsRevision) useVoice.setState({ error: voiceError(error) }); }
}

export function bindVoice(): () => void {
  void refreshVoiceViews();
  void refreshJobs();
  const doc = useDoc.subscribe((next, previous) => { if (next.project !== previous.project) { void refreshVoiceViews(); void refreshJobs(); } });
  const prefs = usePreferences.subscribe((next, previous) => { if (next.saved.voice !== previous.saved.voice) { void refreshVoiceViews(); void refreshJobs(); } });
  return () => { revision++; jobsRevision++; doc(); prefs(); };
}

let edits: Promise<void> = Promise.resolve();
export function operatorEdit(op: object): Promise<void> {
  edits = edits.then(async () => {
    try {
      const answer = await invoke<ProjectState>("operator_edit", { project: useDoc.getState().project, op });
      dispatch({ type: "apply", ...answer });
    } catch (error) { sayError(voiceError(error)); }
  });
  return edits;
}

export async function addOperator(name: string): Promise<void> {
  await operatorEdit({ op: "addTrack", name });
}

export async function addOrder(trackId: string, at: number): Promise<void> {
  await operatorEdit({ op: "addOrder", trackId, at });
  const track = useDoc.getState().project.timelines.flatMap((t) => t.tracks).find((t) => t.id === trackId);
  if (track?.kind.type === "operator") {
    const order = [...track.kind.orders].reverse().find((o) => o.at === at);
    if (order) select({ kind: "operator", id: order.id });
  }
}

export async function loadVoices(): Promise<void> {
  useVoice.setState({ loading: true, listError: null });
  try { useVoice.setState({ voices: await invoke<Voice[]>("voice_list") }); }
  catch (error) { useVoice.setState({ listError: voiceError(error) }); }
  finally { useVoice.setState({ loading: false }); }
}

async function attachJob(jobId: string, recover: boolean, docKey: number): Promise<void> {
  for (let attempt = 0; attempt < 3; attempt++) {
    await edits;
    await flushDocumentSync();
    if (useUi.getState().docKey !== docKey) return;
    const current = useDoc.getState().project;
    let answer: ProjectState;
    try { answer = await invoke<ProjectState>("voice_commit", { project: current, jobId, recover }); }
    catch (error) { if (useUi.getState().docKey === docKey && useDoc.getState().project !== current) continue; throw error; }
    if (useUi.getState().docKey !== docKey) return;
    if (useDoc.getState().project !== current) continue;
    dispatch({ type: "apply", ...answer });
    await analyse(answer.project.assets.filter((a) => a.id === jobId));
    return;
  }
  throw { code: "obsolete" };
}

export async function generateOrder(id: string): Promise<void> {
  if (useVoice.getState().busy) return;
  useVoice.setState({ busy: id, error: null });
  const docKey = useUi.getState().docKey;
  try {
    await edits;
    await flushDocumentSync();
    const jobId = await invoke<string>("voice_generate", { project: useDoc.getState().project, id });
    await attachJob(jobId, false, docKey);
  } catch (error) { useVoice.setState({ error: voiceError(error) }); }
  finally { useVoice.setState({ busy: null }); await refreshJobs(); }
}

export async function recoverJob(jobId: string): Promise<void> {
  if (useVoice.getState().busy) return;
  useVoice.setState({ busy: "recovery", error: null });
  try { await attachJob(jobId, true, useUi.getState().docKey); }
  catch (error) { useVoice.setState({ error: voiceError(error) }); }
  finally { useVoice.setState({ busy: null }); await refreshJobs(); }
}

export async function retrySave(): Promise<void> {
  if (useVoice.getState().busy) return;
  useVoice.setState({ busy: "saving", error: null });
  try { await invoke("voice_retry_save"); }
  catch (error) { useVoice.setState({ error: voiceError(error) }); }
  finally { useVoice.setState({ busy: null }); await refreshJobs(); }
}

/** Resolve a selected operator item for menus; generation and timing remain backend-owned. */
export function selectedOperatorTrack() {
  const selection = useUi.getState().selection;
  if (selection?.kind !== "operator") return null;
  return useDoc.getState().project.timelines.flatMap((t) => t.tracks).find((t) => t.kind.type === "operator" && t.kind.orders.some((o) => o.id === selection.id)) ?? null;
}
