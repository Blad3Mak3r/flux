import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useRef, useState } from "react";
import symbol from "../assets/fxtc-symbol.svg";
import { useT } from "../i18n";
import { CloseIcon } from "./icons";
import { updateCheck, updateInstall } from "./updateApi";

/** What the update row shows. The backend decides every transition; this only names the state. */
type UpdateState =
  | { step: "idle" }
  | { step: "checking" }
  | { step: "current" }
  | { step: "available"; version: string }
  | { step: "installing" }
  | { step: "failed"; code: string };

/** Who made Flux and which build this is. Mounted while shown; Escape or the X unmounts it. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateState>({ step: "idle" });

  useEffect(() => {
    ref.current?.showModal();
    // The version of the bundle, as set in tauri.conf.json. Without the desktop shell there is none.
    getVersion().then(setVersion, () => undefined);
  }, []);

  const check = () => {
    setUpdate({ step: "checking" });
    updateCheck().then(
      (found) => setUpdate(found ? { step: "available", version: found } : { step: "current" }),
      (error) => setUpdate({ step: "failed", code: String(error) }),
    );
  };

  const install = () => {
    setUpdate({ step: "installing" });
    // On success the app restarts, so there is no state to come back to.
    updateInstall().catch((error) => setUpdate({ step: "failed", code: String(error) }));
  };

  return (
    <dialog ref={ref} className="dialog about" onClose={onClose} aria-labelledby="about-title">
      <button type="button" className="icon-button" aria-label={t("about.close")} title={t("about.close")} onClick={() => ref.current?.close()}>
        <CloseIcon />
      </button>
      <img src={symbol} alt="" draggable={false} />
      <h2 id="about-title">{t("app.title")}</h2>
      {version ? <p>{t("about.version", { version })}</p> : null}
      <p>{t("about.developer", { name: "BladeMaker" })}</p>
      <div className="about-update">
        <p role="status">{updateMessage(update, t)}</p>
        {update.step === "available" ? (
          <button type="button" className="button primary" onClick={install}>
            {t("about.update.install")}
          </button>
        ) : (
          <button type="button" className="button" onClick={check} disabled={update.step === "checking" || update.step === "installing"}>
            {t("about.update.check")}
          </button>
        )}
      </div>
    </dialog>
  );
}

function updateMessage(update: UpdateState, t: ReturnType<typeof useT>): string {
  switch (update.step) {
    case "idle":
      return "";
    case "checking":
      return t("about.update.checking");
    case "current":
      return t("about.update.current");
    case "available":
      return t("about.update.available", { version: update.version });
    case "installing":
      return t("about.update.installing");
    case "failed":
      if (update.code === "updatePlaying") return t("about.update.playing");
      return t("about.update.failed", { detail: update.code });
  }
}
