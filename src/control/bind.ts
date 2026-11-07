import { setView } from "../state/ui";
import { play } from "../transport/playback";
import { onWindowAction } from "./api";

/**
 * The backend runs what the remote control asks for, except what only the window can do: opening
 * the output to play (the window owns it) and switching the view. It says so with an event, and
 * `play` and `setView` keep their own rules (the view cannot change while the show plays). Call
 * once from the root; call the result to stop.
 */
export function bindRemoteControl(): () => void {
  return onWindowAction((action) => {
    if (action === "play") void play();
    else setView(action === "viewLive" ? "live" : "edit");
  });
}
