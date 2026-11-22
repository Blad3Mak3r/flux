// How long a clip is on the timeline, which the lanes, the scroll width and the magnet all need.

import type { AudioClip, VideoClip } from "../project/types";
import type { VideoProbe } from "../video/api";

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

/**
 * Length in timeline frames of a video clip: the frames of the video it shows (its trim, else the
 * rest of the video), at the video's own rate, else the guess while the video has not been read.
 */
export function videoClipLength(clip: VideoClip, video: VideoProbe | undefined, fps: number, guess: number): number {
  if (!video) return guess;
  const frames = clip.length ?? Math.max(0, video.frames - (clip.offset ?? 0));
  return Math.round(((frames * video.rateDen) / video.rateNum) * fps);
}
