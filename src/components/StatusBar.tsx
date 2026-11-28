import { useT } from "../i18n";
import { openMedia, useMedia } from "../media/store";
import "../media/media.css";
import { activeTimeline } from "../state/doc";
import { useDoc } from "../state/store";
import { useUi } from "../state/ui";
import { formatFrames } from "../timecode";
import { selectSpan, useReadout } from "../timeline/layout";
import { VjTip } from "../vj/VjScreen";

export function StatusBar() {
  const t = useT();
  const message = useUi((ui) => ui.message);
  const { frameRate: rate, start } = useDoc((doc) => activeTimeline(doc).timecode);
  const hover = useReadout((readout) => readout.hover);
  const span = useReadout(selectSpan);
  const vj = useDoc((doc) => doc.project.mode === "vj");
  const offline = useMedia((media) => media.missing);
  return (
    <footer className="statusbar">
      <p className="message" data-tone={message.tone} role={message.tone === "error" ? "alert" : "status"}>
        {"key" in message ? t(message.key, message.values) : message.text}
      </p>
      <p className="measure">
        {offline > 0 && (
          <button type="button" className="media-offline" title={t("media.offlineOpen")} onClick={openMedia}>
            <i aria-hidden="true" />
            {t("media.offlineCount", { count: offline })}
          </button>
        )}
        {vj && "key" in message && message.key === "status.ready" && <VjTip />}
        {span !== null && span > 0 && (
          <span>
            {t("status.span")} <b>{formatFrames(span, rate)}</b> ({t("status.frames", { count: span })})
          </span>
        )}
        {hover !== null && (
          <span>
            {t("status.cursor")} <b>{formatFrames(start + hover, rate)}</b>
          </span>
        )}
      </p>
    </footer>
  );
}
