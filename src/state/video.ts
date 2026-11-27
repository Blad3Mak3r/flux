// Reading the videos of the project in the backend: their real length, for the timeline to draw.

import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { errorMessage } from "../project/api";
import type { VideoAsset } from "../project/types";
import {
  CONVERT_PROGRESS,
  cancelConvert,
  convertToHap,
  hapAvailable,
  probeVideo,
  replaceVideo,
  videoSound,
  videoThumbnails,
} from "../video/api";
import type { ConvertProgress } from "../video/api";
import { useMedia, withoutOffline } from "../media/store";
import { dispatch, useDoc } from "./store";
import { say, sayError } from "./ui";

/** The videos the backend could not read, by id, with why: their clips show it. */
export const useVideoErrors = create<Record<string, string>>(() => ({}));

/**
 * Asks the backend about each video; one it cannot read is reported and drawn at the guessed length.
 * A VJ project needs neither the waveform nor the tempo of a video's sound.
 */
export async function probeVideos(all: VideoAsset[]): Promise<void> {
  const vj = useDoc.getState().project.mode === "vj";
  // An offline video is not an error: the media page lists it, and it is read once it is relinked.
  const assets = await withoutOffline(all);
  await Promise.all(
    assets.map((asset) =>
      probeVideo(asset)
        .then((info) => dispatch({ type: "video", id: asset.id, info }))
        .then(() => Promise.all([loadThumbs(asset), vj ? undefined : loadSound(asset)]))
        .catch((err) => {
          useVideoErrors.setState({ [asset.id]: errorMessage(err) });
          sayError(`${asset.name}: ${errorMessage(err)}`);
        }),
    ),
  );
}

/** Fetches the thumbnails of a video once probed; without them its clip is drawn plain. */
async function loadThumbs(asset: VideoAsset): Promise<void> {
  const { data, ...strip } = await videoThumbnails(asset);
  const blob = await (await fetch(data)).blob();
  dispatch({ type: "thumbs", id: asset.id, strip: { ...strip, url: URL.createObjectURL(blob) } });
}

/** Fetches the waveform and tempo of a video's sound; a video without sound has neither. */
async function loadSound(asset: VideoAsset): Promise<void> {
  const sound = await videoSound(asset);
  if (!sound) return;
  dispatch({
    type: "audio",
    id: asset.id,
    info: {
      seconds: sound.durationSeconds,
      peaks: { values: sound.peaks, perSecond: sound.peaksPerSecond },
      bpm: sound.bpm ?? undefined,
    },
  });
}

const asked = new Set<string>();

/** Loads the waveform of a video's sound once, for a VJ clip that shows it; `probeVideos` skips it for a VJ project. */
export function ensureSound(asset: VideoAsset): void {
  if (asked.has(asset.id) || useDoc.getState().audio[asset.id] || useMedia.getState().offline.has(asset.id)) return;
  asked.add(asset.id);
  loadSound(asset).catch((err) => {
    asked.delete(asset.id);
    sayError(`${asset.name}: ${errorMessage(err)}`);
  });
}

/** The conversion to HAP running, if any: the video's id and how many of its frames are done. */
export interface Converting {
  asset: string;
  done: number;
  total: number;
  cancelled: boolean;
}

export const useConvert = create<{ converting: Converting | null; available: boolean | null }>(() => ({
  converting: null,
  available: null,
}));

/** Asks once whether HAP can be written here, for the inspector to offer it. */
export function checkHap(): void {
  if (useConvert.getState().available !== null) return;
  hapAvailable()
    .then((available) => useConvert.setState({ available }))
    .catch(() => useConvert.setState({ available: false }));
}

/** Converts a video to HAP and, once done, shows the copy in every clip of the video (one step of undo). */
export async function convertVideo(asset: VideoAsset): Promise<void> {
  if (useConvert.getState().converting) return;
  useConvert.setState({ converting: { asset: asset.id, done: 0, total: 0, cancelled: false } });
  const unlisten = listen<ConvertProgress>(CONVERT_PROGRESS, ({ payload }) => {
    const converting = useConvert.getState().converting;
    if (converting?.asset === payload.asset) useConvert.setState({ converting: { ...converting, ...payload } });
  });
  try {
    const path = await convertToHap(asset);
    const { project, active } = await replaceVideo(useDoc.getState().project, asset.id, path);
    dispatch({ type: "apply", project, active });
    const added = (project.videos ?? []).filter((video) => !useDoc.getState().video[video.id]);
    say("status.convertedHap", { name: asset.name });
    void probeVideos(added);
  } catch (err) {
    if (useConvert.getState().converting?.cancelled) say("status.convertCancelled", { name: asset.name });
    else sayError(`${asset.name}: ${errorMessage(err)}`);
  } finally {
    useConvert.setState({ converting: null });
    void unlisten.then((stop) => stop());
  }
}

/** Stops the conversion running. */
export function cancelVideoConvert(): void {
  const converting = useConvert.getState().converting;
  if (!converting) return;
  useConvert.setState({ converting: { ...converting, cancelled: true } });
  void cancelConvert();
}
