// The names the VJ screen gives what has none of its own: a layer by its place, a clip by its video or generator.

import type { Translate } from "../i18n";
import type { VideoAsset, VjClip, VjGrid, VjGroup, VjLayer } from "../project/types";

export const layerName = (t: Translate, grid: VjGrid, layer: VjLayer) =>
  layer.name ?? t("vj.layer.default", { n: grid.layers.indexOf(layer) + 1 });

export const groupName = (t: Translate, grid: VjGrid, group: VjGroup) =>
  group.name ?? t("vj.group.default", { n: (grid.groups ?? []).indexOf(group) + 1 });

export function clipName(t: Translate, clip: VjClip, videos: VideoAsset[]): string {
  if (clip.name) return clip.name;
  if (clip.generator) return t(`vj.generator.${clip.generator.kind}`);
  return videos.find((video) => video.id === clip.asset)?.name ?? "";
}
