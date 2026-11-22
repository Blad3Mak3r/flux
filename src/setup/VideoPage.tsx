import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useT } from "../i18n";
import { SelectField } from "../components/fields";
import { errorMessage } from "../project/api";
import { sayError } from "../state/ui";

// Mirrors src-tauri/src/video_output.rs.
interface Display {
  id: string;
  width: number;
  height: number;
  main: boolean;
}
interface VideoOutputStatus {
  displays: Display[];
  chosen: string | null;
  open: boolean;
  missing: boolean;
  error: string | null;
}

const OFF = "";
/** Displays come and go while the page is open: their list is asked for again this often. */
const REFRESH_MS = 2000;

/** The display the video output opens on. The backend opens, places and draws the window. */
export function VideoPage() {
  const t = useT();
  const [status, setStatus] = useState<VideoOutputStatus | null>(null);

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
  }, []);

  if (!status) return <p className="empty-note">{t("setup.video.loading")}</p>;

  const choose = (display: string) =>
    invoke<VideoOutputStatus>("video_output_choose", { display: display === OFF ? null : display }).then(
      setStatus,
      (err) => sayError(errorMessage(err)),
    );
  const label = (display: Display, index: number) => {
    const name = t("setup.video.choice", { n: index + 1, width: display.width, height: display.height });
    return display.main ? t("setup.video.thisScreen", { name }) : name;
  };
  const chosen = status.chosen ?? OFF;

  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.video.intro")}</p>
      <SelectField label={t("setup.video.display")} value={chosen} hint={t("setup.video.mainNote")} onChange={(id) => void choose(id)}>
        <option value={OFF}>{t("setup.video.off")}</option>
        {status.displays.map((display, index) => (
          <option key={display.id} value={display.id}>
            {label(display, index)}
          </option>
        ))}
        {status.missing && <option value={chosen}>{t("setup.video.disconnected")}</option>}
      </SelectField>
      <p className="field-note" role="status">
        {status.error
          ? t("setup.video.error", { error: status.error })
          : status.missing
            ? t("setup.video.missing")
            : status.open
              ? t("setup.video.open")
              : chosen !== OFF
                ? t("setup.video.closed")
                : t("setup.video.offNote")}
      </p>
    </div>
  );
}
