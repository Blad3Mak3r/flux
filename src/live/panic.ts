import { fail } from "../state/fail";
import type { Translate } from "../i18n/messages";
import { useDoc } from "../state/store";
import { say, sayError } from "../state/ui";
import { stop } from "../transport/playback";
import { midiPanic } from "./api";

/** Stops the transport and silences the project's MIDI gear; the backend sends the messages. */
export async function panic(t: Translate): Promise<void> {
  try {
    await stop();
    const report = await midiPanic(useDoc.getState().project);
    if (report.failed.length > 0) sayError(t("panic.failed", { ports: report.failed.join(", ") }));
    else say("panic.sent", { count: report.sent });
  } catch (err) {
    fail(err);
  }
}
