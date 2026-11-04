// Reading the audio the backend decodes after an import or an open: real length and waveform.

import { analyzeAudio } from "../audio/api";
import { errorMessage } from "../project/api";
import type { AudioAsset } from "../project/types";
import { dispatch } from "./store";
import { say, sayError } from "./ui";

let generation = 0;

/** A document that is replaced while its audio is still being read must not receive the result. */
export const forgetAnalyses = () => {
  generation += 1;
};

export function analyse(assets: AudioAsset[]): void {
  const current = generation;
  if (assets.length === 0) return;
  say("status.analysing");
  let pending = assets.length;
  for (const asset of assets) {
    analyzeAudio(asset)
      .then((info) => {
        if (generation !== current) return;
        dispatch({
          type: "audio",
          id: asset.id,
          info: {
            seconds: info.durationSeconds,
            peaks: { values: info.peaks, perSecond: info.peaksPerSecond },
          },
        });
      })
      .catch((err) => generation === current && sayError(`${asset.name}: ${errorMessage(err)}`))
      .finally(() => {
        pending -= 1;
        if (pending === 0 && generation === current) say("status.analysed");
      });
  }
}
