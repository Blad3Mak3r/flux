import { open } from "@tauri-apps/plugin-dialog";
import { TextField } from "../components/fields";
import { useT } from "../i18n";
import { changePreferences, usePreferences } from "../preferences/store";

/** Where Output > Snapshot saves its PNGs, and whether each is also added to the media. */
export function SnapshotPage() {
  const t = useT();
  const dir = usePreferences((p) => p.saved.snapshotDir) ?? null;
  const media = usePreferences((p) => p.saved.snapshotMedia) ?? false;
  const save = (change: object) => void changePreferences(change).catch(() => undefined);
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.snapshots.intro")}</p>
      <TextField
        label={t("setup.video.snapshotDir")}
        monospace
        value={dir ?? t("setup.video.snapshotDefault")}
        onCommit={(value) => save({ op: "snapshotDir", dir: value })}
      />
      <div className="button-row">
        <button
          type="button"
          className="button"
          onClick={() =>
            void open({ directory: true }).then((chosen) => {
              if (typeof chosen === "string") save({ op: "snapshotDir", dir: chosen });
            })
          }
        >
          {t("setup.video.snapshotChoose")}
        </button>
        {dir && (
          <button type="button" className="button" onClick={() => save({ op: "snapshotDir", dir: null })}>
            {t("setup.video.snapshotReset")}
          </button>
        )}
      </div>
      <label className="check-row">
        <input type="checkbox" checked={media} onChange={(event) => save({ op: "snapshotMedia", on: event.target.checked })} />
        <span>{t("setup.video.snapshotMedia")}</span>
      </label>
    </div>
  );
}
