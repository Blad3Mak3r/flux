// A video clip on its lane: its thumbnails, the waveform of its sound and its handles.

import { useT } from "../i18n";
import type { Track, VideoClip } from "../project/types";
import { MediaProgressBar } from "../media/progress";
import { focusMedia, useMedia } from "../media/store";
import { useDoc } from "../state/store";
import { isSelected, useUi } from "../state/ui";
import { videoClipLength } from "../timeline/clip";
import type { VideoStrip } from "../video/api";
import { fadePoints } from "../audio/fade";
import { frameX } from "../timeline/scale";
import { ClipWave } from "./ClipWave";
import { useDrag, useHandle, ClipHandle, fadeHandleLeft } from "./laneGestures";

/** A video clip: moved by dragging it, trimmed by its edges, faded by the squares at its top corners, cut with S at the playhead. The backend makes each edit. */
export function VideoBlock({
  track,
  clip,
  guessLength,
  rowHeight,
  fps,
  pxPerFrame,
}: {
  track: Track;
  clip: VideoClip;
  guessLength: number;
  rowHeight: number;
  fps: number;
  pxPerFrame: number;
}) {
  const t = useT();
  const selected = useUi((ui) => isSelected(ui, clip.id));
  const probe = useDoc((doc) => doc.video[clip.asset]);
  const offline = useMedia((media) => media.offline.has(clip.asset));
  const name = useDoc((doc) => clip.name ?? doc.project.videos?.find((video) => video.id === clip.asset)?.name ?? clip.asset);
  const length = videoClipLength(clip, probe, fps, guessLength);
  const { shown, handlers: pointer } = useDrag(clip.id, "video", clip.start, length, pxPerFrame, track, offline ? () => focusMedia(clip.asset) : undefined);
  const end = clip.start + length;
  const trimStart = useHandle(clip.id, "trimStart", clip.start, pxPerFrame, true);
  const trimEnd = useHandle(clip.id, "trimEnd", end, pxPerFrame, true);
  const fadeIn = useHandle(clip.id, "fadeIn", clip.start + (clip.fadeIn ?? 0), pxPerFrame, true);
  const fadeOut = useHandle(clip.id, "fadeOut", end - (clip.fadeOut ?? 0), pxPerFrame, true);
  const left = trimStart.at + (shown - clip.start);
  const width = Math.max(24, (trimEnd.at - trimStart.at) * pxPerFrame);
  const fadeInPx = Math.min(width, Math.max(0, (fadeIn.at - trimStart.at) * pxPerFrame));
  const fadeOutPx = Math.min(width, Math.max(0, (trimEnd.at - fadeOut.at) * pxPerFrame));
  const strip = useDoc((doc) => doc.thumbs[clip.asset]);
  const sound = useDoc((doc) => doc.audio[clip.asset]);
  const offset = (clip.offset ?? 0) + (trimStart.at - clip.start);
  // With sound, the pictures take the top of the clip and the waveform the bottom.
  const inner = rowHeight - 14;
  const waveHeight = sound?.peaks ? Math.round(inner * 0.4) : 0;
  return (
    <div
      className="clip clip-video"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-offline={offline || undefined}
      style={{ left: frameX(left, pxPerFrame), width, height: rowHeight - 12 }}
      {...pointer}
    >
      {strip && (
        <VideoThumbTiles
          strip={strip}
          offset={offset}
          width={width}
          height={inner - waveHeight}
          fps={fps}
          pxPerFrame={pxPerFrame}
        />
      )}
      {sound?.peaks && (
        <span className="clip-video-wave" style={{ height: waveHeight }}>
          <ClipWave
            seed={clip.asset}
            width={width}
            height={waveHeight}
            origin={frameX(left, pxPerFrame)}
            color="currentColor"
            peaks={sound.peaks}
            pxPerSecond={pxPerFrame * fps}
            startSeconds={offset / fps}
          />
        </span>
      )}
      {(fadeInPx > 0 || fadeOutPx > 0) && (
        <svg className="clip-fades" width={width} height={inner} aria-hidden="true">
          {fadeInPx > 0 && <polyline points={fadePoints("linear", "in", fadeInPx, inner)} />}
          {fadeOutPx > 0 && (
            <polyline transform={`translate(${width - fadeOutPx} 0)`} points={fadePoints("linear", "out", fadeOutPx, inner)} />
          )}
        </svg>
      )}
      <span className="clip-label">
        {name}
        {offline && <span className="clip-offline">{t("media.state.offline")}</span>}
        {sound?.bpm !== undefined && <span className="clip-bpm">{sound.bpm} BPM</span>}
        {probe && (
          <span className="clip-bpm">
            {probe.width}×{probe.height} {probe.codec}
          </span>
        )}
      </span>
      <MediaProgressBar asset={clip.asset} />
      <ClipHandle edge="start" label={t("clip.trimStart", { name })} handlers={trimStart.handlers} />
      <ClipHandle edge="end" label={t("clip.trimEnd", { name })} handlers={trimEnd.handlers} />
      <ClipHandle
        edge="fadeIn"
        label={t("clip.fadeIn", { name })}
        style={{ left: fadeHandleLeft(fadeInPx, width) }}
        handlers={fadeIn.handlers}
      />
      <ClipHandle
        edge="fadeOut"
        label={t("clip.fadeOut", { name })}
        style={{ left: fadeHandleLeft(width - fadeOutPx, width) }}
        handlers={fadeOut.handlers}
      />
    </div>
  );
}

/** Most tiles a clip draws, so a long clip zoomed far in stays light. */
const MAX_TILES = 400;

/**
 * The thumbnails along a video clip, each as tall as the clip and showing the video where it starts
 * on the timeline. `offset` is the first frame of the video the clip shows, in timeline frames.
 */
function VideoThumbTiles({
  strip,
  offset,
  width,
  height,
  fps,
  pxPerFrame,
}: {
  strip: VideoStrip;
  offset: number;
  width: number;
  height: number;
  fps: number;
  pxPerFrame: number;
}) {
  const tile = (strip.width * height) / strip.height;
  const count = Math.min(MAX_TILES, Math.ceil(width / tile));
  return (
    <div className="clip-thumbs" aria-hidden="true">
      {Array.from({ length: count }, (_, k) => {
        const seconds = (offset + (k * tile) / pxPerFrame) / fps;
        const index = Math.min(strip.count - 1, Math.max(0, Math.floor(seconds / strip.seconds)));
        return (
          <span
            key={k}
            style={{
              width: tile,
              height,
              backgroundImage: `url(${strip.url})`,
              backgroundSize: `${tile * strip.count}px ${height}px`,
              backgroundPosition: `${-index * tile}px 0`,
            }}
          />
        );
      })}
    </div>
  );
}
