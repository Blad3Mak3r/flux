import symbol from "../assets/fxtc-symbol.svg";
import { useI18n } from "../i18n";

/** The FXTC symbol standing in for the "Flux Timecode" wordmark. With `onClick` it is a button. */
export function BrandMark({ onClick }: { onClick?: () => void }) {
  const { t } = useI18n();
  const image = <img src={symbol} alt={t("app.title")} draggable={false} />;
  return (
    <p className="brand" data-tauri-drag-region>
      {onClick ? (
        <button type="button" className="brand-button" onClick={onClick} title={t("about.open")}>
          {image}
        </button>
      ) : (
        image
      )}
    </p>
  );
}
