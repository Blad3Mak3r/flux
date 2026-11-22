// Reading the videos of the project in the backend: their real length, for the timeline to draw.

import { errorMessage } from "../project/api";
import type { VideoAsset } from "../project/types";
import { probeVideo, videoThumbnails } from "../video/api";
import { dispatch } from "./store";
import { sayError } from "./ui";

/** Asks the backend about each video; one it cannot read is reported and drawn at the guessed length. */
export function probeVideos(assets: VideoAsset[]): Promise<void> {
  return Promise.all(
    assets.map((asset) =>
      probeVideo(asset)
        .then((info) => dispatch({ type: "video", id: asset.id, info }))
        .then(() => loadThumbs(asset))
        .catch((err) => sayError(`${asset.name}: ${errorMessage(err)}`)),
    ),
  ).then(() => undefined);
}

/** Fetches the thumbnails of a video once probed; without them its clip is drawn plain. */
async function loadThumbs(asset: VideoAsset): Promise<void> {
  const { data, ...strip } = await videoThumbnails(asset);
  const blob = await (await fetch(data)).blob();
  dispatch({ type: "thumbs", id: asset.id, strip: { ...strip, url: URL.createObjectURL(blob) } });
}
