// What the grid, the inspector and the media browser share: the empty list a store selector answers, and the drag payloads.

import type { VideoAsset, VjGeneratorKind, VjAutopilot } from "../project/types";

/** One empty list for a project without videos, so a store selector answers the same value each time. */
export const NO_VIDEOS: VideoAsset[] = [];

export const AUTOPILOTS: VjAutopilot[] = ["off", "next", "previous", "random"];

export const DRAG = "application/x-flux-vj";

/** What a layer dragged by its name carries: its id, to drop in or out of a group or at another place. */
export const DRAG_LAYER = "application/x-flux-vj-layer";

export type Dragged = { clip: string } | { asset: string } | { generator: VjGeneratorKind };
