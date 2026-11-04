import { useEffect, useRef } from "react";
import { useT } from "../i18n";

/**
 * Asks before something irreversible. Cancel has the focus, so Enter never confirms by accident,
 * and the confirm button is only ever reached on purpose.
 */
export function ConfirmDialog({
  open,
  title,
  body,
  detail,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  body: string;
  /** What exactly is affected, such as a file path. */
  detail?: string;
  confirmLabel: string;
  onConfirm: () => void;
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
    <dialog ref={ref} className="dialog" onCancel={onCancel} aria-labelledby="confirm-title">
      <h2 id="confirm-title">{title}</h2>
      {detail && <p className="dialog-detail">{detail}</p>}
      <p>{body}</p>
      <div className="button-row">
        <button type="button" className="button" onClick={onCancel} autoFocus>
          {t("dialog.cancel")}
        </button>
        <button type="button" className="button danger" onClick={onConfirm}>
          {confirmLabel}
        </button>
      </div>
    </dialog>
  );
}
