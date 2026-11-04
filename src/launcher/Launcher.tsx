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

type Shown = Step["state"] | "waiting" | "skipped";

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

// React runs effects twice in development; the checks and the window change must happen once.
let started = false;

function Launcher() {
  const t = useT();
  const [steps, setSteps] = useState<Partial<Record<StepId, Step>>>({});
  const [finished, setFinished] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

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
        const report = await Promise.race([startupRun().catch(() => undefined), sleep(GIVE_UP_MS)]);
        if (Array.isArray(report)) report.forEach(apply);
        setFinished(true);
        await sleep(Math.max(0, MIN_VISIBLE_MS - (Date.now() - begun)));
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

  return (
    <main className="launcher" data-tauri-drag-region>
      <BrandMark />
      <DotMatrix text="00:00:00:00" label={t("readout.label")} />
      <ul className="startup" aria-live="polite">
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
