import symbol from "../assets/fxtc-symbol.svg";
import { useI18n } from "../i18n";

/** The FLUX symbol and wordmark. With `onClick` it is a button. */
export function BrandMark({ onClick }: { onClick?: () => void }) {
  const { t } = useI18n();
  const image = <><img src={symbol} alt="" draggable={false} /><span>{t("app.title")}</span></>;
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
