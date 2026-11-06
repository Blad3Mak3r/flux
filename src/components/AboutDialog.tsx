import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useRef, useState } from "react";
import symbol from "../assets/fxtc-symbol.svg";
import { useT } from "../i18n";
import { CloseIcon } from "./icons";

/** Who made Flux and which build this is. Mounted while shown; Escape or the X unmounts it. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    ref.current?.showModal();
    // The version of the bundle, as set in tauri.conf.json. Without the desktop shell there is none.
    getVersion().then(setVersion, () => undefined);
  }, []);

  return (
    <dialog ref={ref} className="dialog about" onClose={onClose} aria-labelledby="about-title">
      <button type="button" className="icon-button" aria-label={t("about.close")} title={t("about.close")} onClick={() => ref.current?.close()}>
        <CloseIcon />
      </button>
      <img src={symbol} alt="" draggable={false} />
      <h2 id="about-title">{t("app.title")}</h2>
      {version ? <p>{t("about.version", { version })}</p> : null}
      <p>{t("about.developer", { name: "BladeMaker" })}</p>
    </dialog>
  );
}
