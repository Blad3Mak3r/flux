import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useT } from "../i18n";
import { SelectField, TextField } from "../components/fields";
import { errorMessage } from "../project/api";
import { sayError } from "../state/ui";
import { useDoc } from "../state/store";
import { MAIN_SCREEN, compositionSet, screenEdit, useComposition } from "../video/mapping";
import { MappingEditor } from "./MappingEditor";

// Mirrors src-tauri/src/video_output.rs.
interface Display {
  id: string;
  width: number;
  height: number;
  main: boolean;
}
interface ScreenStatus {
  id: string;
  name: string;
  chosen: string | null;
  open: boolean;
  missing: boolean;
  error: string | null;
}
interface VideoOutputStatus {
  displays: Display[];
  /** The main screen first. */
  screens: ScreenStatus[];
}

const OFF = "";
/** Displays come and go while the page is open: their list is asked for again this often. */
const REFRESH_MS = 2000;

/**
 * The screens of the video output, each on a display of its own with its own slices. The backend
 * opens, places and draws their windows.
 */
export function VideoPage() {
  const t = useT();
  const [status, setStatus] = useState<VideoOutputStatus | null>(null);
  const [selected, setSelected] = useState(MAIN_SCREEN);
  // A screen added, renamed or removed is asked for again at once rather than at the next refresh.
  const screens = useDoc((doc) => doc.project.output.screens);
  const composition = useComposition();

  useEffect(() => {
    let alive = true;
    const read = () =>
      invoke<VideoOutputStatus>("video_output_status").then(
        (next) => alive && setStatus(next),
        (err) => sayError(errorMessage(err)),
      );
    void read();
    const timer = window.setInterval(() => void read(), REFRESH_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [screens]);

  if (!status) return <p className="empty-note">{t("setup.video.loading")}</p>;

  const current =
    status.screens.find((s) => s.id === selected) ?? status.screens[0];
  if (!current) return null;
  const choose = (display: string) =>
    invoke<VideoOutputStatus>("video_output_choose", {
      screen: current.id,
      display: display === OFF ? null : display,
    }).then(setStatus, (err) => sayError(errorMessage(err)));
  const label = (display: Display, index: number) => {
    const name = t("setup.video.choice", {
      n: index + 1,
      width: display.width,
      height: display.height,
    });
    return display.main ? t("setup.video.thisScreen", { name }) : name;
  };
  const chosen = current.chosen ?? OFF;
  const screen =
    status.displays.find((display) => display.id === chosen) ?? null;

  return (
    <>
      <div className="setup-fields">
        <p className="setup-intro">{t("setup.video.intro")}</p>
        <h4 id="composition-title">{t("setup.video.composition")}</h4>
        <fieldset className="mapping-values" aria-labelledby="composition-title">
          <TextField
            label={t("setup.video.field.width")}
            inputMode="numeric"
            monospace
            value={String(composition.width)}
            onCommit={(width) => void compositionSet(width, String(composition.height))}
          />
          <TextField
            label={t("setup.video.field.height")}
            inputMode="numeric"
            monospace
            value={String(composition.height)}
            onCommit={(height) => void compositionSet(String(composition.width), height)}
          />
        </fieldset>
        <p className="field-note">
          {t(composition.set ? "setup.video.compositionSet" : "setup.video.compositionAuto")}
        </p>
        {composition.set && (
          <div className="button-row">
            <button type="button" className="button" onClick={() => void compositionSet("", "")}>
              {t("setup.video.compositionReset")}
            </button>
          </div>
        )}
        <div
          className="screen-tabs"
          role="group"
          aria-label={t("setup.video.screens")}
        >
          {status.screens.map((s) => (
            <button
              key={s.id}
              type="button"
              className="screen-tab"
              aria-pressed={s.id === current.id}
              data-open={s.open}
              onClick={() => setSelected(s.id)}
            >
              <span className="screen-dot" aria-hidden="true" />
              {s.name}
            </button>
          ))}
          <button
            type="button"
            className="button"
            onClick={() =>
              void screenEdit({ op: "add" }).then(() => {
                // The new screen is the last one: it is shown at once.
                const list = useDoc.getState().project.output.screens ?? [];
                const added = list[list.length - 1];
                if (added) setSelected(added.id);
              })
            }
          >
            {t("setup.video.addScreen")}
          </button>
        </div>
        <TextField
          label={t("setup.video.screenName")}
          value={current.name}
          onCommit={(name) =>
            void screenEdit({ op: "rename", id: current.id, name })
          }
        />
        <SelectField
          label={t("setup.video.display")}
          value={chosen}
          hint={t("setup.video.mainNote")}
          onChange={(id) => void choose(id)}
        >
          <option value={OFF}>{t("setup.video.off")}</option>
          {status.displays.map((display, index) => (
            <option key={display.id} value={display.id}>
              {label(display, index)}
            </option>
          ))}
          {current.missing && (
            <option value={chosen}>{t("setup.video.disconnected")}</option>
          )}
        </SelectField>
        <p className="field-note" role="status">
          {current.error
            ? t("setup.video.error", { error: current.error })
            : current.missing
              ? t("setup.video.missing")
              : current.open
                ? t("setup.video.open")
                : chosen !== OFF
                  ? t("setup.video.closed")
                  : t("setup.video.offNote")}
        </p>
        {current.id !== MAIN_SCREEN && (
          <div className="button-row">
            <button
              type="button"
              className="button danger"
              onClick={() => {
                setSelected(MAIN_SCREEN);
                void screenEdit({ op: "remove", id: current.id });
              }}
            >
              {t("setup.video.removeScreen")}
            </button>
          </div>
        )}
      </div>
      <MappingEditor screenId={current.id} screen={screen} />
    </>
  );
}
