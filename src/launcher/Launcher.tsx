import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { DotMatrix } from "../components/DotMatrix";
import { BrandMark } from "../components/BrandMark";
import { hasNativeWindow } from "../components/WindowControls";
import { I18nProvider, useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { errorMessage } from "../project/api";
import { showWelcome, startupRun } from "../welcome/api";
import type { Step, StepId } from "../welcome/api";
import { UPDATE_START, runUpdate } from "./updater";
import type { UpdateControl, UpdateView } from "./updater";
import { formatMegabytes, percent } from "./updateProgress";

const STEPS: { id: StepId; name: MessageKey }[] = [
  { id: "audio", name: "launcher.audio" },
  { id: "midi", name: "launcher.midi" },
  { id: "ltc", name: "launcher.ltc" },
  { id: "mtc", name: "launcher.mtc" },
];

/** Long enough to read the list, short enough not to feel like a wait. */
const MIN_VISIBLE_MS = 700;
/** A driver that never answers must not keep the app from opening. */
const GIVE_UP_MS = 10_000;
/** A failed update is left on screen long enough to be read before Flux opens anyway. */
const FAILURE_VISIBLE_MS = 2500;

type Shown = Step["state"] | "waiting" | "skipped";

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

// React runs effects twice in development; the checks and the window change must happen once.
let started = false;

// The user's "Skip" while the update downloads: `control` stops the install, `skipRequested`
// stops the wait.
const control: UpdateControl = { skipped: false };
let skipNow: () => void = () => undefined;
const skipRequested = new Promise<void>((resolve) => (skipNow = resolve));

const isMac = () => /Mac/.test(navigator.userAgent);

function updateLamp({ state }: UpdateView) {
  if (state === "current") return "on";
  if (state === "failed") return "fault";
  return state === "downloading" || state === "installing" ? "hold" : "off";
}

function Launcher() {
  const t = useT();
  const [steps, setSteps] = useState<Partial<Record<StepId, Step>>>({});
  const [finished, setFinished] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateView>(UPDATE_START);

  useEffect(() => {
    if (!hasNativeWindow()) return;
    let stop: (() => void) | undefined;
    let cancelled = false;
    const apply = (step: Step) => setSteps((all) => ({ ...all, [step.id]: step }));

    listen<Step>("startup-step", (event) => apply(event.payload))
      .then((off) => (cancelled ? off() : (stop = off)))
      .catch(() => undefined);

    if (!started) {
      started = true;
      void (async () => {
        const begun = Date.now();
        const checks = Promise.race([startupRun().catch(() => undefined), sleep(GIVE_UP_MS)]);
        let latest = UPDATE_START;
        const updating = runUpdate((patch) => {
          latest = { ...latest, ...patch };
          setUpdate(latest);
        }, control);
        // Both run together; the app opens once the update step has nothing left to do.
        const [report] = await Promise.all([checks, Promise.race([updating, skipRequested])]);
        if (Array.isArray(report)) report.forEach(apply);
        setFinished(true);
        const readFailure = latest.state === "failed" ? FAILURE_VISIBLE_MS : 0;
        await sleep(Math.max(readFailure, MIN_VISIBLE_MS - (Date.now() - begun)));
        await showWelcome().catch((err) => setFailure(errorMessage(err)));
      })();
    }

    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  const shown = (id: StepId): Shown => steps[id]?.state ?? (finished ? "skipped" : "waiting");
  const detail = (id: StepId): string => {
    const step = steps[id];
    if (step?.message) return step.message;
    if (step?.count == null) return "";
    return step.count === 0 ? t("launcher.none") : t("launcher.found", { count: step.count });
  };

  const updateDetail = (): string => {
    switch (update.state) {
      case "checking":
        return t("launcher.update.checking");
      case "current":
        return update.version ? `v${update.version}` : "";
      case "downloading":
        return update.version ?? "";
      case "installing":
        return t(isMac() ? "launcher.update.restart" : "launcher.update.authorize");
      case "offline":
        return t("launcher.update.offline");
      case "failed":
        return update.error ?? "";
      case "skipped":
        return t(`launcher.update.${update.reason ?? "user"}`);
    }
  };

  const skipUpdate = () => {
    control.skipped = true;
    setUpdate((view) => ({ ...view, state: "skipped", reason: "user" }));
    skipNow();
  };

  const { progress } = update;
  const downloading = update.state === "downloading" || update.state === "installing";
  const done = update.state === "installing" ? 100 : percent(progress);
  const megabytes = formatMegabytes(progress.downloaded);

  return (
    <main className="launcher" data-tauri-drag-region>
      <BrandMark />
      <DotMatrix text="00:00:00:00" label={t("readout.label")} />
      <ul className="startup" aria-live="polite">
        <li>
          <span className="lamp" data-state={updateLamp(update)} />
          <span className="lamp-name">{t("launcher.update")}</span>
          <span className="lamp-status">{t(`launcher.update.state.${update.state}`)}</span>
          <span className="lamp-detail" title={updateDetail()}>{updateDetail()}</span>
        </li>
        {STEPS.map(({ id, name }) => {
          const state = shown(id);
          return (
            <li key={id}>
              <span className="lamp" data-state={state === "ready" ? "on" : state === "fault" ? "fault" : "off"} />
              <span className="lamp-name">{t(name)}</span>
              <span className="lamp-status">{t(`launcher.state.${state}`)}</span>
              <span className="lamp-detail" title={detail(id)}>{detail(id)}</span>
            </li>
          );
        })}
      </ul>
      <div className="startup-foot">
        {downloading && (
          <>
            <div
              className="update-bar"
              role="progressbar"
              aria-label={t("launcher.update.download")}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={done ?? undefined}
            >
              <span style={{ width: `${done ?? 0}%` }} />
            </div>
            <span className="update-figures">
              {progress.total === null
                ? t("launcher.update.progressUnknown", { done: megabytes })
                : t("launcher.update.progress", {
                    done: megabytes,
                    total: formatMegabytes(progress.total),
                    percent: done ?? 0,
                  })}
            </span>
            {update.state === "downloading" && (
              <button type="button" className="button" onClick={skipUpdate}>
                {t("launcher.update.skip")}
              </button>
            )}
          </>
        )}
        {update.state === "failed" && (
          <p className="message" data-tone="error" role="alert">
            {t("launcher.update.failedNote", { version: update.version ? `v${update.version}` : "" })}
          </p>
        )}
      </div>
      {failure && (
        <p className="message" data-tone="error" role="alert">
          {t("launcher.failed")} {failure}
        </p>
      )}
    </main>
  );
}

export default function LauncherView() {
  return (
    <I18nProvider>
      <Launcher />
    </I18nProvider>
  );
}
