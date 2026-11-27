import { useT } from "../i18n";
import { relinkMedia } from "./actions";
import { useMedia } from "./store";
import "./media.css";

/** The file a linked clip plays, for the inspector: its path, and a way to relink it when it is gone. */
export function MediaFile({ id }: { id: string }) {
  const t = useT();
  const item = useMedia((media) => media.items.find((found) => found.id === id));
  const busy = useMedia((media) => media.busy);
  if (!item || item.state === "embedded") return null;
  return (
    <div className="media-file" data-state={item.state}>
      <dl className="facts">
        <dt>{t("media.col.path")}</dt>
        <dd className="media-path" title={item.path ?? undefined}>{item.path}</dd>
        {item.state === "offline" && (
          <>
            <dt>{t("media.col.state")}</dt>
            <dd className="media-fault">{t("media.state.offline")}</dd>
          </>
        )}
      </dl>
      {item.state === "offline" && (
        <button type="button" className="button" disabled={busy} onClick={() => void relinkMedia(item)}>
          {t("media.relink")}
        </button>
      )}
    </div>
  );
}
