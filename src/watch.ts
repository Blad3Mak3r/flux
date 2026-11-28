import { Channel, invoke } from "@tauri-apps/api/core";

/**
 * Opens a watch on the backend: `command` (a `*_watch` command taking `onValue`'s channel as `channel`) sends
 * the state down it whenever it changes. Returns the function that ends the watch.
 */
export function watch<T>(command: string, onValue: (value: T) => void, channel = "onState"): () => void {
  const lane = new Channel<T>();
  let stopped = false;
  lane.onmessage = (value) => {
    if (!stopped) onValue(value);
  };
  void invoke(command, { [channel]: lane }).catch(() => undefined);
  return () => {
    stopped = true;
    void invoke("watch_stop", { id: lane.id }).catch(() => undefined);
  };
}
