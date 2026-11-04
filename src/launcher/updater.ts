// The launcher's update step: look for a newer Flux, download it and install it before the app
// opens. Nothing here may stop Flux from starting: every failure ends in a state the launcher
// shows and then moves past.

import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { errorMessage } from "../project/api";
import { PROGRESS_START, stepProgress } from "./updateProgress";
import type { Progress } from "./updateProgress";

export type UpdateState =
  | "checking"
  | "current"
  | "downloading"
  | "installing"
  | "offline"
  | "failed"
  | "skipped";

export interface UpdateView {
  state: UpdateState;
  /** The version being installed, or the one running when there is nothing to install. */
  version: string | null;
  progress: Progress;
  /** Why a `failed` update failed. */
  error: string | null;
  /** Why a step is `skipped`: this is a development build, or the user said so. */
  reason: "development" | "user" | null;
}

export const UPDATE_START: UpdateView = {
  state: "checking",
  version: null,
  progress: PROGRESS_START,
  error: null,
  reason: null,
};

/** A venue's network must not hold the app back for long. */
export const CHECK_TIMEOUT_MS = 5000;

/** The launcher sets `skipped` when the user asks to open Flux without the update. */
export interface UpdateControl {
  skipped: boolean;
}

/** True once the step has nothing more to wait for, so the launcher can open the app. */
export const isUpdateSettled = (state: UpdateState) =>
  state !== "checking" && state !== "downloading" && state !== "installing";

/**
 * Runs the step, reporting each change. Resolves when it is settled; on a successful install it
 * restarts the app instead, so it does not resolve at all.
 */
export async function runUpdate(report: (patch: Partial<UpdateView>) => void, control: UpdateControl): Promise<void> {
  if (import.meta.env.DEV) {
    report({ state: "skipped", reason: "development" });
    return;
  }
  const running = await getVersion().catch(() => null);

  const skip = () => {
    report({ state: "skipped", reason: "user" });
  };

  let update;
  try {
    update = await check({ timeout: CHECK_TIMEOUT_MS });
  } catch {
    report({ state: "offline", version: running });
    return;
  }
  if (!update) {
    report({ state: "current", version: running });
    return;
  }

  report({ state: "downloading", version: update.version, progress: PROGRESS_START });
  try {
    let progress = PROGRESS_START;
    await update.download((event) => {
      progress = stepProgress(progress, event);
      report({ progress });
    });
  } catch (err) {
    report({ state: "failed", version: running, error: errorMessage(err) });
    return;
  }
  // Installing is never started once the user chose to open Flux as it is.
  if (control.skipped) return skip();

  report({ state: "installing" });
  try {
    await update.install();
    await relaunch();
  } catch (err) {
    report({ state: "failed", version: running, error: errorMessage(err) });
  }
}
