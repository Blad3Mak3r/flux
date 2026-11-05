import { invoke } from "@tauri-apps/api/core";
import type { AudioAsset } from "../project/types";

// Mirrors src-tauri/src/audio_commands.rs.

export interface AudioInfo {
  durationSeconds: number;
  /** Sample rate of the file itself, before the engine converts it to 48 kHz. */
  sourceRate: number;
  peaksPerSecond: number;
  /** `[min0, max0, min1, max1, ...]` as signed bytes: -128 and 127 are full scale. */
  peaks: number[];
}

/** The tempo of an imported file in BPM, or `null` when it shows no steady pulse. Decodes it if needed. */
export function detectBpm(asset: AudioAsset): Promise<number | null> {
  return invoke<number | null>("audio_detect_bpm", { asset });
}

/** Decodes an imported file in the backend: its real length and its waveform. */
export function analyzeAudio(asset: AudioAsset): Promise<AudioInfo> {
  return invoke<AudioInfo>("audio_analyze", { asset });
}
