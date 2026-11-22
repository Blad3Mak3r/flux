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
