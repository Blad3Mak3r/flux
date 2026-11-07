// How long a clip is on the timeline, which the lanes, the scroll width and the magnet all need.

import type { AudioClip } from "../project/types";

/** Length shown for a clip whose audio length is not known yet (10 s at the project rate). */
export const GUESS_SECONDS = 10;

/**
 * Length in frames of a clip: the trim the backend wrote, else the whole audio once it has been
 * read, else the guess. (The backend always writes a length when it moves the start.)
 */
export function clipLength(
  clip: AudioClip,
  audio: { seconds: number } | undefined,
  fps: number,
  guess: number,
): number {
  return clip.length ?? (audio ? Math.round(audio.seconds * fps) : guess);
}
