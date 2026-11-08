import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../project/api";
import { hasNativeWindow } from "../components/WindowControls";
import type { ProjectState } from "../project/types";
import { dispatch, useDoc } from "../state/store";
import { sayError, useUi } from "../state/ui";
import { useLive } from "../live/store";
import { displayClock } from "../transport/smooth";
import { realFps } from "../timecode";
import { useTransport } from "../transport/store";
import { prepareSetlist, leaveSetlist } from "../transport/playback";
import type { ShowAction, ShowEdit, ShowState } from "./types";
import { useShow } from "./store";

export async function editShow(op: ShowEdit): Promise<void> {
  try {
    const state = await invoke<ProjectState>("show_edit", { project: useDoc.getState().project, op });
    dispatch({ type: "apply", ...state });
    if (useShow.getState().state) await leaveSetlist();
  } catch (error) { sayError(errorMessage(error)); }
}
export async function showAction(action: ShowAction): Promise<void> {
  try { await invoke("show_action", { action }); } catch (error) { sayError(errorMessage(error)); }
}
export async function prepareShow(id: string): Promise<void> {
  useShow.setState({ preparing: true });
  try { await prepareSetlist(id); await refresh(); } catch (error) { sayError(errorMessage(error)); }
  finally { useShow.setState({ preparing: false }); }
}
export const closeShow = async () => {
  try { await leaveSetlist(); } catch (error) { sayError(errorMessage(error)); }
};
async function refresh(): Promise<void> {
  const state = await invoke<ShowState | null>("show_state");
  const before = useShow.getState().state;
  useShow.setState({ state });
  if (state) {
    if (!before) useUi.setState({view: "live"});
    if (useDoc.getState().active !== state.timelineId) dispatch({ type: "active", id: state.timelineId });
    if (before?.entryId !== state.entryId) useLive.setState({ cues: state.cues, ready: true });
    useLive.setState({ state: state.live });
    useTransport.setState({ position: state.position });
    displayClock.feed(state.position, performance.now(), realFps(state.frameRate), state.start);
  }
}
export function bindShow(): () => void {
  if (!hasNativeWindow()) return () => {};
  let stopped = false;
  let timer: ReturnType<typeof setTimeout>;
  const poll = async () => {
    if (stopped) return;
    try { await refresh(); } catch { /* Window startup and shutdown can briefly have no IPC. */ }
    if (!stopped) timer = setTimeout(poll, 100);
  };
  void poll();
  const onKey = (event: KeyboardEvent) => {
    if (event.key !== "Enter" || event.repeat || event.ctrlKey || event.metaKey || event.altKey || useUi.getState().view !== "live") return;
    if (event.target instanceof Element && event.target.closest("input, textarea, select, button, [contenteditable=true], [role=dialog]")) return;
    if (useShow.getState().state?.tick.go === "none" || !useShow.getState().state) return;
    event.preventDefault();
    void showAction({ type: "go" });
  };
  window.addEventListener("keydown", onKey);
  return () => { stopped = true; clearTimeout(timer); window.removeEventListener("keydown", onKey); };
}
