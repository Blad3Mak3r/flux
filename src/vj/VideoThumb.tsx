// A clip's picture and the state of its video: the thumbnail strip, and what shows over it while the file is read, missing or unreadable.

import type { CSSProperties, ReactNode } from "react";
import { useT } from "../i18n";
import { useMedia } from "../media/store";
import { useVideoErrors } from "../state/video";
import type { VjClip, VjGenerator } from "../project/types";
import { useDoc } from "../state/store";
import type { VideoStrip } from "../video/api";
import { pad2 } from "../format";
import { swatch } from "./generators";

/** Whether a video can be shown yet: `offline` when its file is gone, `failed` when the backend cannot read it, `processing` while it reads it. */
export function useVideoState(asset: string | undefined): "offline" | "failed" | "processing" | null {
  const error = useVideoErrors((errors) => (asset ? errors[asset] : undefined));
  const offline = useMedia((media) => asset !== undefined && media.offline.has(asset));
  const ready = useDoc((doc) => !asset || (asset in doc.video && asset in doc.thumbs));
  if (offline) return "offline";
  if (error) return "failed";
  return ready ? null : "processing";
}

/** The state of a clip's video over its picture: importing, processing or unreadable. */
export function VideoState({ state, asset }: { state: "importing" | "processing" | "failed" | "offline"; asset?: string }) {
  const t = useT();
  const error = useVideoErrors((errors) => (asset === undefined ? undefined : errors[asset]));
  return (
    <span className={`vj-state ${state}`} title={error}>
      {state !== "failed" && state !== "offline" && <i className="vj-spin" aria-hidden="true" />}
      {t(`vj.state.${state}`)}
    </span>
  );
}

/** `mm:ss` of a count of seconds. */
export function clock(seconds: number): string {
  const whole = Math.max(0, Math.ceil(seconds));
  return `${pad2(Math.floor(whole / 60))}:${pad2(whole % 60)}`;
}

/** The thumbnails of a clip's video; a generator has none. */
export const stripOf = (thumbs: Record<string, VideoStrip>, clip: VjClip | undefined) =>
  clip?.asset ? thumbs[clip.asset] : undefined;

/** One picture of a video's thumbnail strip, at `position` (0 to 1) of the video, or what a generator draws. */
export function Thumb({ strip, generator, position = 0.3, children, className }: {
  strip: VideoStrip | undefined;
  generator?: VjGenerator;
  position?: number;
  children?: ReactNode;
  className?: string;
}) {
  const style: CSSProperties = generator ? swatch(generator) : {};
  if (strip && !generator) {
    const index = Math.min(strip.count - 1, Math.max(0, Math.floor(position * strip.count)));
    style.backgroundImage = `url(${strip.url})`;
    style.backgroundSize = `${strip.count * 100}% 100%`;
    style.backgroundPosition = strip.count > 1 ? `${(index / (strip.count - 1)) * 100}% 0` : "0 0";
  }
  return <div className={`vj-thumb ${className ?? ""}`} style={style}>{children}</div>;
}

export function MediaState({ asset }: { asset: string }) {
  const state = useVideoState(asset);
  return state && <VideoState state={state} asset={asset} />;
}
