/**
 * Wraps a call that a gesture repeats faster than the backend needs (dragging the playhead or a
 * fader): while one is in flight only the newest value waits, so the last one always arrives and
 * the ones in between are skipped.
 */
export function latestOnly<V>(
  send: (value: V) => Promise<unknown>,
  onError: (error: unknown) => void,
): (value: V) => void {
  let busy = false;
  // Boxed, so that a falsy value such as frame 0 still counts as waiting.
  let waiting: { value: V } | null = null;

  const run = (value: V) => {
    busy = true;
    send(value)
      .catch(onError)
      .finally(() => {
        busy = false;
        if (!waiting) return;
        const next = waiting.value;
        waiting = null;
        run(next);
      });
  };

  return (value) => {
    if (busy) waiting = { value };
    else run(value);
  };
}
