import { open } from "@tauri-apps/plugin-dialog";
import { TextField } from "../components/fields";
import { useT } from "../i18n";
import { changePreferences, usePreferences } from "../preferences/store";

/** Where Composition > Start recording saves its videos. */
export function RecordingsPage() {
  const t = useT();
  const dir = usePreferences((p) => p.saved.recordingsDir) ?? null;
  const save = (change: object) => void changePreferences(change).catch(() => undefined);
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.recordings.intro")}</p>
      <TextField
        label={t("setup.recordings.dir")}
        monospace
        value={dir ?? t("setup.recordings.default")}
        onCommit={(value) => save({ op: "recordingsDir", dir: value })}
      />
      <div className="button-row">
        <button
          type="button"
          className="button"
          onClick={() =>
            void open({ directory: true }).then((chosen) => {
              if (typeof chosen === "string") save({ op: "recordingsDir", dir: chosen });
            })
          }
        >
          {t("setup.recordings.choose")}
        </button>
        {dir && (
          <button type="button" className="button" onClick={() => save({ op: "recordingsDir", dir: null })}>
            {t("setup.recordings.reset")}
          </button>
        )}
      </div>
    </div>
  );
}
