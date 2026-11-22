// Reading the videos of the project in the backend: their real length, for the timeline to draw.

import { errorMessage } from "../project/api";
import type { VideoAsset } from "../project/types";
import { probeVideo } from "../video/api";
import { dispatch } from "./store";
import { sayError } from "./ui";

/** Asks the backend about each video; one it cannot read is reported and drawn at the guessed length. */
export function probeVideos(assets: VideoAsset[]): Promise<void> {
  return Promise.all(
    assets.map((asset) =>
      probeVideo(asset)
        .then((info) => dispatch({ type: "video", id: asset.id, info }))
        .catch((err) => sayError(`${asset.name}: ${errorMessage(err)}`)),
    ),
  ).then(() => undefined);
}
