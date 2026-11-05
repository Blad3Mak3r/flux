// Reading the audio the backend decodes after an import or an open: real length and waveform.

import { analyzeAudio, detectBpm } from "../audio/api";
import { errorMessage } from "../project/api";
import type { AudioAsset } from "../project/types";
import { edit } from "./actions";
import { setAssetBpm } from "./edit";
import { dispatch, useDoc } from "./store";
import { say, sayError } from "./ui";

let generation = 0;

/** A document that is replaced while its audio is still being read must not receive the result. */
export const forgetAnalyses = () => {
  generation += 1;
};

/** Reads the length and waveform of each asset; resolves when all of them were tried. */
export function analyse(assets: AudioAsset[]): Promise<void> {
  const current = generation;
  if (assets.length === 0) return Promise.resolve();
  say("status.analysing");
  let pending = assets.length;
  const reads = assets.map((asset) =>
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
      }),
  );
  return Promise.all(reads).then(() => undefined);
}

/**
 * Finds the tempo of one asset and keeps it with the asset. Audio with no steady pulse gives
 * `null` and leaves the asset as it was: the user can still type a tempo in the inspector. With
 * `report`, the status line says what was found, for a detection the user asked for.
 */
export async function detectTempo(asset: AudioAsset, report = false, derived = false): Promise<void> {
  const current = generation;
  try {
    const bpm = await detectBpm(asset);
    if (generation !== current) return;
    // A tempo found for a file that was opened is not the user's edit: it neither enters the undo
    // history nor makes the document unsaved.
    if (bpm !== null && derived) dispatch({ type: "derivedBpm", id: asset.id, bpm });
    else if (bpm !== null) edit(setAssetBpm(useDoc.getState().project, asset.id, bpm));
    if (report) {
      if (bpm === null) say("status.tempoNone", { name: asset.name });
      else say("status.tempoFound", { name: asset.name, bpm });
    }
  } catch (err) {
    if (generation === current) sayError(`${asset.name}: ${errorMessage(err)}`);
  }
}

/**
 * Detects the tempo of the assets that have none. Returns when every one was tried. With
 * `derived` (a project that was just opened) they go one at a time, so a show of many songs does
 * not decode them all at once.
 */
export async function detectTempos(assets: AudioAsset[], derived = false): Promise<void> {
  const missing = assets.filter((asset) => asset.bpm === undefined);
  if (derived) {
    for (const asset of missing) await detectTempo(asset, false, true);
  } else {
    await Promise.all(missing.map((asset) => detectTempo(asset)));
  }
}
