import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import symbol from "../assets/fxtc-symbol.svg";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { CloseIcon, RefreshIcon } from "./icons";
import { DOCS_URL, ISSUES_URL, REPO_URL, releaseUrl } from "./links";
import { updateCheck, updateInstall } from "./updateApi";
import { hasNativeWindow } from "./WindowControls";

/** What the update panel shows. The backend decides every transition; this only names the state. */
type UpdateState =
  | { step: "idle" }
  | { step: "checking" }
  | { step: "current" }
  | { step: "available"; version: string }
  | { step: "installing"; version: string }
  | { step: "failed"; code: string; version?: string };

/** The panel's lamp, as on the faceplate: off, lit, a cue to act on, or a fault. */
const LAMP: Record<UpdateState["step"], "off" | "on" | "cue" | "fault"> = {
  idle: "off",
  checking: "off",
  current: "on",
  available: "cue",
  installing: "cue",
  failed: "fault",
};

/** Who made Flux and which build this is, and the way to the next one. Mounted while shown; Escape or the X unmounts it. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);
  const native = hasNativeWindow();
  const [version, setVersion] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateState>({ step: "idle" });

  const check = () => {
    setUpdate({ step: "checking" });
    updateCheck().then(
      (found) => setUpdate(found ? { step: "available", version: found } : { step: "current" }),
      (error) => setUpdate({ step: "failed", code: String(error) }),
    );
  };

  useEffect(() => {
    ref.current?.showModal();
    // The version of the bundle, as set in tauri.conf.json. Without the desktop shell there is none.
    getVersion().then(setVersion, () => undefined);
    // Opening About is asking which build this is: answer whether it is the latest too.
    if (native) check();
  }, [native]);

  const install = (next: string) => {
    setUpdate({ step: "installing", version: next });
    // On success the app restarts, so there is no state to come back to.
    updateInstall().catch((error) => setUpdate({ step: "failed", code: String(error), version: next }));
  };

  const busy = update.step === "checking" || update.step === "installing";
  const open = (url: string) => () => void openUrl(url);

  return (
    <dialog ref={ref} className="dialog about" onClose={onClose} aria-labelledby="about-title">
      <button type="button" className="icon-button about-close" aria-label={t("about.close")} title={t("about.close")} onClick={() => ref.current?.close()}>
        <CloseIcon />
      </button>

      <header className="about-head">
        <img src={symbol} alt="" draggable={false} />
        <h2 id="about-title">{t("app.title")}</h2>
        <p className="about-tagline">{t("about.tagline")}</p>
        <p className="about-meta">
          {version ? <span className="about-version">{t("about.version", { version })}</span> : null}
          <span>{t("about.developer", { name: "BladeMaker" })}</span>
        </p>
      </header>

      <section className="about-update" aria-labelledby="about-update-legend" data-step={update.step} aria-busy={busy}>
        <h3 id="about-update-legend" className="legend">
          {t("about.update.legend")}
        </h3>
        <p className="about-update-status" role="status">
          <span className="lamp" data-state={LAMP[update.step]} aria-hidden="true" />
          <span>{updateMessage(update, t, native)}</span>
        </p>
        {update.step === "failed" && update.code !== "updatePlaying" ? (
          <p className="about-update-detail" title={update.code}>{update.code}</p>
        ) : null}
        {update.step === "installing" ? <progress aria-label={t("about.update.installing")} /> : null}
        <div className="about-update-actions">
          {update.step === "available" || (update.step === "failed" && update.version) ? (
            <>
              <button type="button" className="button primary" onClick={() => install(update.version!)}>
                {t(update.step === "failed" ? "about.update.retryInstall" : "about.update.install", { version: update.version! })}
              </button>
              <button type="button" className="button" onClick={open(releaseUrl(update.version!))}>
                {t("about.update.notes")}
              </button>
            </>
          ) : (
            <button type="button" className="button" onClick={check} disabled={!native || busy}>
              <RefreshIcon />
              {t(checkLabel(update.step))}
            </button>
          )}
        </div>
      </section>

      <nav className="about-links" aria-label={t("about.links")}>
        <button type="button" className="about-link" disabled={!native} onClick={open(DOCS_URL)}>{t("menu.docs")}</button>
        <button type="button" className="about-link" disabled={!native} onClick={open(REPO_URL)}>GitHub</button>
        <button type="button" className="about-link" disabled={!native} onClick={open(ISSUES_URL)}>{t("menu.reportIssue")}</button>
      </nav>
    </dialog>
  );
}

function checkLabel(step: UpdateState["step"]): MessageKey {
  if (step === "checking") return "about.update.checkingShort";
  if (step === "installing") return "about.update.installingShort";
  if (step === "idle") return "about.update.check";
  return step === "failed" ? "about.update.retry" : "about.update.checkAgain";
}

function updateMessage(update: UpdateState, t: ReturnType<typeof useT>, native: boolean): string {
  switch (update.step) {
    case "idle":
      return t(native ? "about.update.idle" : "about.update.desktopOnly");
    case "checking":
      return t("about.update.checking");
    case "current":
      return t("about.update.current");
    case "available":
      return t("about.update.available", { version: update.version });
    case "installing":
      return t("about.update.installingVersion", { version: update.version });
    case "failed":
      if (update.code === "updatePlaying") return t("about.update.playing");
      return t("about.update.failed");
  }
}
