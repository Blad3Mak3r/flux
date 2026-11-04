import symbol from "../assets/fxtc-symbol.svg";
import { useI18n } from "../i18n";

/** The FXTC symbol standing in for the "Flux Timecode" wordmark. */
export function BrandMark() {
  const { t } = useI18n();
  return (
    <p className="brand" data-tauri-drag-region>
      <img src={symbol} alt={t("app.title")} draggable={false} />
    </p>
  );
}
