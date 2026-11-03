import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { FrameRate } from "../project/types";
import { useStore } from "../store";
import { formatFrames } from "../timecode";
import { readoutStore } from "../timeline/layout";

/** A status line is either a translatable message or raw text from the backend (an error). */
export type Message =
  | { key: MessageKey; values?: Record<string, string | number>; tone: "info" }
  | { text: string; tone: "error" };

export function StatusBar({
  message,
  rate,
  start,
}: {
  message: Message;
  rate: FrameRate;
  start: number;
}) {
  const t = useT();
  const { hover, spanA, spanB } = useStore(readoutStore);
  const span = spanA !== null && spanB !== null ? Math.abs(spanB - spanA) : null;
  return (
    <footer className="statusbar">
      <p className="message" data-tone={message.tone} role={message.tone === "error" ? "alert" : "status"}>
        {"key" in message ? t(message.key, message.values) : message.text}
      </p>
      <p className="measure">
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
