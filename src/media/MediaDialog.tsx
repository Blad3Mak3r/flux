// The media page: every file the project plays, which are missing, and how to find them again.
// The backend looks at the files and does the finding and copying; this lists its answer.

import { useEffect, useRef, useState } from "react";
import { useT } from "../i18n";
import { CloseIcon, FolderIcon } from "../components/icons";
import { useDoc } from "../state/store";
import { collectMedia, relinkMedia, revealMedia, searchMediaFolder } from "./actions";
import { closeMedia, useMedia } from "./store";
import "./media.css";

/** Opens from File > Media… or the offline count in the status bar; closes with its button or Escape. */
export function MediaDialog() {
  const open = useMedia((media) => media.open);
  return open ? <MediaPage /> : null;
}

function MediaPage() {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);
  const items = useMedia((media) => media.items);
  const missing = useMedia((media) => media.missing);
  const busy = useMedia((media) => media.busy);
  const progress = useMedia((media) => media.progress);
  const saved = useDoc((doc) => doc.path !== null);
  const [onlyMissing, setOnlyMissing] = useState(false);

  useEffect(() => {
    ref.current?.showModal();
  }, []);

  const shown = onlyMissing ? items.filter((item) => item.state === "offline") : items;
  const percent = progress && progress.totalBytes > 0 ? Math.min(100, Math.floor((progress.doneBytes * 100) / progress.totalBytes)) : 0;

  return (
    <dialog ref={ref} className="dialog media" aria-labelledby="media-title" onCancel={closeMedia}>
      <header className="media-head">
        <h2 id="media-title">{t("media.title")}</h2>
        <p className="media-summary" data-tone={missing > 0 ? "fault" : undefined} role="status">
          {items.length === 0
            ? t("media.empty")
            : missing > 0
              ? t("media.summary", { missing, total: items.length })
              : t("media.summaryNone", { total: items.length })}
        </p>
        <label className="media-filter">
          <input type="checkbox" checked={onlyMissing} onChange={(event) => setOnlyMissing(event.target.checked)} />
          {t("media.filter")}
        </label>
        <button type="button" className="icon-button" aria-label={t("media.close")} title={t("media.close")} onClick={closeMedia}>
          <CloseIcon />
        </button>
      </header>
      <div className="media-scroll">
        <table className="media-table">
          <thead>
            <tr>
              <th scope="col">{t("media.col.name")}</th>
              <th scope="col">{t("media.col.kind")}</th>
              <th scope="col">{t("media.col.state")}</th>
              <th scope="col">{t("media.col.path")}</th>
              <th scope="col" className="num">{t("media.col.uses")}</th>
              <th scope="col" />
            </tr>
          </thead>
          <tbody>
            {shown.map((item) => (
              <tr key={item.id} data-state={item.state}>
                <td className="name" title={item.name}>{item.name}</td>
                <td>{t(`media.kind.${item.kind}`)}</td>
                <td className="state">
                  <i className="media-lamp" aria-hidden="true" />
                  {t(`media.state.${item.state}`)}
                </td>
                <td className="path" title={item.path ?? undefined}>{item.path ?? t("media.noFile")}</td>
                <td className="num">{item.uses}</td>
                <td className="row-actions">
                  <button type="button" className="button" disabled={busy} aria-label={t("media.relinkNamed", { name: item.name })} onClick={() => void relinkMedia(item)}>
                    {t("media.relink")}
                  </button>
                  {item.state === "online" && item.path !== null && (
                    <button type="button" className="icon-button" title={t("media.reveal")} aria-label={t("media.reveal")} onClick={() => revealMedia(item.path as string)}>
                      <FolderIcon />
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <footer className="media-foot">
        <button type="button" className="button" disabled={busy || missing === 0} title={t("media.searchNote")} onClick={() => void searchMediaFolder()}>
          {t("media.search")}
        </button>
        <button type="button" className="button" disabled={busy || !saved || items.length === 0} title={saved ? t("media.collectNote") : t("media.error.unsaved")} onClick={() => void collectMedia()}>
          {t("media.collect")}
        </button>
        {progress && (
          <div className="media-progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent} aria-label={t("media.collecting", { percent })}>
            <span style={{ width: `${percent}%` }} />
          </div>
        )}
        {progress && <span className="media-progress-note">{t("media.collecting", { percent })}</span>}
      </footer>
    </dialog>
  );
}
