// The question an import asks when it would overwrite mappings the show has: what each conflict is, and what to do
// about it, for all at once or one by one.

import { useEffect, useRef, useState } from "react";
import { useT } from "../i18n";
import type { VideoAsset } from "../project/types";
import { useDoc } from "../state/store";
import { runImport, useImportAsk } from "./mapFile";
import { sourceLabel, targetLabel } from "./mapModel";

const NO_VIDEOS: VideoAsset[] = [];

export function MapImportDialog() {
  const t = useT();
  const pending = useImportAsk((state) => state.pending);
  const grid = useDoc((doc) => doc.project.vj);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const ref = useRef<HTMLDialogElement>(null);
  // The conflicts left as they are, by their place in the file.
  const [keep, setKeep] = useState<Set<number>>(new Set());
  useEffect(() => setKeep(new Set()), [pending]);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (pending && !dialog.open) dialog.showModal();
    if (!pending && dialog.open) dialog.close();
  }, [pending]);
  const cancel = () => useImportAsk.setState({ pending: null });
  const conflicts = pending?.plan.conflicts ?? [];
  const go = (wipe: boolean, kept: number[]) => pending && void runImport(pending.path, { wipe, keep: kept });
  const toggle = (index: number) => setKeep((was) => {
    const next = new Set(was);
    if (!next.delete(index)) next.add(index);
    return next;
  });
  return (
    <dialog ref={ref} className="dialog vj-import" onCancel={cancel} aria-labelledby="vj-import-title">
      {pending && grid && (
        <>
          <h2 id="vj-import-title">{t("vj.mapfile.conflictTitle", { n: conflicts.length })}</h2>
          <p>{t("vj.mapfile.conflictBody", { taken: pending.plan.taken, skipped: pending.plan.skipped })}</p>
          <ul className="vj-import-list">
            {conflicts.map((conflict) => {
              const kept = keep.has(conflict.index);
              return (
                <li key={conflict.index} className={kept ? "kept" : ""}>
                  <span className="vj-import-src vj-mono">{sourceLabel(conflict.source)}</span>
                  <span className="vj-import-was">{targetLabel(t, conflict.existing, grid, videos)}</span>
                  <span aria-hidden="true">→</span>
                  <span className="vj-import-now">{targetLabel(t, conflict.incoming, grid, videos)}</span>
                  <div className="vj-segs" role="radiogroup" aria-label={sourceLabel(conflict.source)}>
                    <button type="button" role="radio" aria-checked={!kept} className={kept ? "" : "on"} onClick={() => kept && toggle(conflict.index)}>{t("vj.mapfile.overwrite")}</button>
                    <button type="button" role="radio" aria-checked={kept} className={kept ? "on" : ""} onClick={() => !kept && toggle(conflict.index)}>{t("vj.mapfile.keep")}</button>
                  </div>
                </li>
              );
            })}
          </ul>
          <div className="button-row vj-import-actions">
            <button type="button" className="button" onClick={cancel} autoFocus>{t("dialog.cancel")}</button>
            <button type="button" className="button danger" onClick={() => go(true, [])}>{t("vj.mapfile.replaceAll")}</button>
            <button type="button" className="button" onClick={() => go(false, conflicts.map((c) => c.index))}>{t("vj.mapfile.keepExisting")}</button>
            <button type="button" className="button" onClick={() => go(false, [...keep])}>
              {keep.size === 0 ? t("vj.mapfile.overwriteAll") : t("vj.mapfile.apply")}
            </button>
          </div>
        </>
      )}
    </dialog>
  );
}
