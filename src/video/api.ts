import { invoke } from "@tauri-apps/api/core";
import type { VideoAsset } from "../project/types";

// Mirrors src-tauri/src/video_commands.rs.

/** What FFmpeg read from a video: its length in its own frames, its rate as num/den, its size and codec. */
export interface VideoProbe {
  frames: number;
  rateNum: number;
  rateDen: number;
  width: number;
  height: number;
  codec: string;
}

/** Opens a video of the project in the backend. */
export const probeVideo = (asset: VideoAsset) => invoke<VideoProbe>("video_probe", { asset });

/** Thumbnails of a video, as the backend answers: `count` pictures of `width` x `height` side by side
 * in one JPEG (`data`, a data: URL), one every `seconds` seconds of the video. */
export interface VideoThumbs {
  width: number;
  height: number;
  count: number;
  seconds: number;
  data: string;
}

/** The same strip, held by the window as a short blob: URL. */
export type VideoStrip = Omit<VideoThumbs, "data"> & { url: string };

/** Asks the backend for the thumbnails of a video; the first call for a video decodes them. */
export const videoThumbnails = (asset: VideoAsset) => invoke<VideoThumbs>("video_thumbnails", { asset });

/** The sound of a video as the timeline draws it: its waveform and its tempo (`null` with no steady pulse). */
export interface VideoSound {
  durationSeconds: number;
  peaksPerSecond: number;
  /** `[min0, max0, min1, max1, ...]` as signed bytes: -128 and 127 are full scale. */
  peaks: number[];
  bpm: number | null;
}

/** Decodes the sound of a video in the backend; `null` for a video without sound. */
export const videoSound = (asset: VideoAsset) => invoke<VideoSound | null>("video_sound", { asset });
