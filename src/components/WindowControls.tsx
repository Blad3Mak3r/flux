import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useState } from "react";
import { useT } from "../i18n";
import { CloseIcon, MaximizeIcon, MinimizeIcon, RestoreIcon } from "./icons";

/** The native title bar is off, so the window needs its own buttons. Only the desktop app has a window. */
export const hasNativeWindow = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function WindowControls() {
  const t = useT();
  const [maximized, setMaximized] = useState(false);
  const sync = useCallback(() => getCurrentWindow().isMaximized().then(setMaximized, () => undefined), []);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    let stop: (() => void) | undefined;
    let cancelled = false;
    void sync();
    // Maximizing, restoring and snapping all resize the window.
    appWindow
      .onResized(() => void sync())
      .then((off) => (cancelled ? off() : (stop = off)))
      .catch(() => undefined);
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [sync]);

  const appWindow = getCurrentWindow();
  const run = (action: () => Promise<void>) => () => void action().catch(() => undefined);

  return (
    <div className="window-controls" role="group" aria-label={t("window.group")}>
      <button
        type="button"
        className="window-button"
        aria-label={t("window.minimize")}
        title={t("window.minimize")}
        onClick={run(() => appWindow.minimize())}
      >
        <MinimizeIcon />
      </button>
      <button
        type="button"
        className="window-button"
        aria-label={maximized ? t("window.restore") : t("window.maximize")}
        title={maximized ? t("window.restore") : t("window.maximize")}
        onClick={run(() => appWindow.toggleMaximize().then(sync))}
      >
        {maximized ? <RestoreIcon /> : <MaximizeIcon />}
      </button>
      <button
        type="button"
        className="window-button"
        data-role="close"
        aria-label={t("window.close")}
        title={t("window.close")}
        onClick={run(() => appWindow.close())}
      >
        <CloseIcon />
      </button>
    </div>
  );
}
