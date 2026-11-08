import { useEffect, useRef } from "react";
import { useT } from "../i18n";
import { dismissError, useUi } from "../state/ui";

/** One modal above the current task, including Preferences, without losing its state. */
export function ErrorDialog() {
  const t = useT();
  const error = useUi((ui) => ui.errors[0]);
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (error !== undefined && !dialog.open) dialog.showModal();
    if (error === undefined && dialog.open) dialog.close();
  }, [error]);

  return (
    <dialog ref={ref} className="dialog error-dialog" role="alertdialog"
      aria-labelledby="error-title" aria-describedby="error-body"
      onCancel={(event) => { event.preventDefault(); dismissError(); }}>
      <h2 id="error-title">{t("dialog.error.title")}</h2>
      <p id="error-body">{error}</p>
      <div className="button-row">
        <button type="button" className="button primary" autoFocus onClick={dismissError}>
          {t("dialog.ok")}
        </button>
      </div>
    </dialog>
  );
}
