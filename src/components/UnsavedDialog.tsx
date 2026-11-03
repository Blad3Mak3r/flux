import { useEffect, useRef } from "react";
import { useT } from "../i18n";

/** Asks before work is thrown away. A modal is right here: nothing else may happen meanwhile. */
export function UnsavedDialog({
  name,
  open,
  onSave,
  onDiscard,
  onCancel,
}: {
  name: string;
  open: boolean;
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog ref={ref} className="dialog" onCancel={onCancel} aria-labelledby="unsaved-title">
      <h2 id="unsaved-title">{t("dialog.unsaved.title", { name })}</h2>
      <p>{t("dialog.unsaved.body")}</p>
      <div className="button-row">
        <button type="button" className="button primary" onClick={onSave} autoFocus>
          {t("dialog.save")}
        </button>
        <button type="button" className="button" onClick={onDiscard}>
          {t("dialog.discard")}
        </button>
        <button type="button" className="button" onClick={onCancel}>
          {t("dialog.cancel")}
        </button>
      </div>
    </dialog>
  );
}
