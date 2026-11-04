// How far an update download has got. No React and no Tauri, so it can be tested on its own.

export interface Progress {
  downloaded: number;
  /** `null` until the server says how big the file is, or when it never does. */
  total: number | null;
}

/** The events `Update.download` reports. */
export type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

export const PROGRESS_START: Progress = { downloaded: 0, total: null };

export function stepProgress(progress: Progress, event: DownloadEvent): Progress {
  switch (event.event) {
    case "Started":
      return { downloaded: 0, total: event.data.contentLength || null };
    case "Progress":
      return { ...progress, downloaded: progress.downloaded + event.data.chunkLength };
    case "Finished":
      return { ...progress, total: progress.total ?? progress.downloaded };
  }
}

/** Whole percent, or `null` while the size is unknown. */
export function percent({ downloaded, total }: Progress): number | null {
  if (total === null || total <= 0) return null;
  return Math.min(100, Math.floor((downloaded / total) * 100));
}

/** Megabytes (10^6 bytes, as installers are sized) with one decimal. */
export function formatMegabytes(bytes: number): string {
  return (bytes / 1_000_000).toFixed(1);
}
