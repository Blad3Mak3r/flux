// An audio clip on its lane: its waveform, fades, envelope and handles.

import { useRef } from "react";
import { useT } from "../i18n";
import type { AudioClip, Track } from "../project/types";
import { nameClip } from "../state/actions";
import { MediaProgressBar } from "../media/progress";
import { focusMedia, useMedia } from "../media/store";
import { useDoc } from "../state/store";
import { isSelected, menuAtPointer, rename, select, useUi } from "../state/ui";
import { clipLength } from "../timeline/clip";
import { fadePoints } from "../audio/fade";
import { frameX } from "../timeline/scale";
import { ClipWave } from "./ClipWave";
import { EnvelopeLayer, addEnvelopePoint } from "./EnvelopeLayer";
import { InlineName } from "./InlineName";
import { audioMenu } from "./menus";
import { useDrag, useHandle, ClipHandle, fadeHandleLeft } from "./laneGestures";

export function ClipBlock({
  track,
  clip,
  name,
  guessLength,
  muted,
  rowHeight,
  fps,
  tint,
  pxPerFrame,
}: {
  track: Track;
  clip: AudioClip;
  name: string;
  /** Length shown while the audio has not been read. */
  guessLength: number;
  muted: boolean;
  rowHeight: number;
  /** Real frames per second, to turn frames on screen into seconds of audio. */
  fps: number;
  /** Colour and mute state of the track, which `currentColor` follows. */
  tint: string;
  pxPerFrame: number;
}) {
  const t = useT();
  // Each clip asks the stores for its own slice: a selection or a decoded waveform elsewhere
  // does not re-render it.
  const selected = useUi((ui) => isSelected(ui, clip.id));
  const renaming = useUi((ui) => ui.renaming?.kind === "clip" && ui.renaming.id === clip.id);
  const labelRef = useRef<HTMLSpanElement>(null);
  const audio = useDoc((doc) => doc.audio[clip.asset]);
  const offline = useMedia((media) => media.offline.has(clip.asset));
  const bpm = useDoc((doc) => doc.project.assets.find((asset) => asset.id === clip.asset)?.bpm);
  const length = clipLength(clip, audio, fps, guessLength);
  const { shown, handlers: pointer } = useDrag(clip.id, "clip", clip.start, length, pxPerFrame, track, offline ? () => focusMedia(clip.asset) : undefined);
  const end = clip.start + length;
  const trimStart = useHandle(clip.id, "trimStart", clip.start, pxPerFrame);
  const trimEnd = useHandle(clip.id, "trimEnd", end, pxPerFrame);
  const fadeIn = useHandle(clip.id, "fadeIn", clip.start + (clip.fadeIn ?? 0), pxPerFrame);
  const fadeOut = useHandle(clip.id, "fadeOut", end - (clip.fadeOut ?? 0), pxPerFrame);
  // While a handle is dragged the box follows it; the waveform keeps its place on the timeline.
  const left = trimStart.at + (shown - clip.start);
  const width = Math.max(24, (trimEnd.at - trimStart.at) * pxPerFrame);
  const waveWidth = Math.max(24, length * pxPerFrame);
  const fadeInPx = Math.min(width, Math.max(0, (fadeIn.at - trimStart.at) * pxPerFrame));
  const fadeOutPx = Math.min(width, Math.max(0, (trimEnd.at - fadeOut.at) * pxPerFrame));
  const height = rowHeight - 12;
  return (
    <div
      className="clip"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-offline={offline || undefined}
      data-muted={muted}
      style={{ left: frameX(left, pxPerFrame), width, height }}
      onContextMenu={(event) => {
        if (!isSelected(useUi.getState(), clip.id)) select({ kind: "clip", id: clip.id });
        menuAtPointer(event, audioMenu(t, track, clip));
      }}
      // The clip holds the pointer while it is pressed, so a double click lands here and not on what
      // is under it: on the name it renames the clip, elsewhere on a selected clip it adds a point.
      onDoubleClick={(event) => {
        const label = labelRef.current?.getBoundingClientRect();
        const onLabel =
          label !== undefined &&
          event.clientX >= label.left &&
          event.clientX <= label.right &&
          event.clientY >= label.top &&
          event.clientY <= label.bottom;
        if (onLabel) {
          rename({ kind: "clip", id: clip.id });
        } else if (selected) {
          const box = event.currentTarget.getBoundingClientRect();
          addEnvelopePoint(
            clip,
            {
              left: box.left + event.currentTarget.clientLeft,
              top: box.top + event.currentTarget.clientTop,
              height,
              frame: left,
              pxPerFrame,
            },
            event.clientX,
            event.clientY,
          );
        }
      }}
      {...pointer}
    >
      <span className="clip-wave-shift" style={{ left: (clip.start - trimStart.at) * pxPerFrame }}>
        <ClipWave
          seed={clip.asset}
          width={waveWidth}
          height={height}
          origin={frameX(clip.start, pxPerFrame)}
          color="currentColor"
          peaks={audio?.peaks}
          pxPerSecond={pxPerFrame * fps}
          startSeconds={(clip.offset ?? 0) / fps}
          tint={tint}
        />
      </span>
      {(fadeInPx > 0 || fadeOutPx > 0) && (
        <svg className="clip-fades" width={width} height={height} aria-hidden="true">
          {fadeInPx > 0 && <polyline points={fadePoints(clip.fadeInCurve ?? "linear", "in", fadeInPx, height)} />}
          {fadeOutPx > 0 && (
            <polyline
              transform={`translate(${width - fadeOutPx} 0)`}
              points={fadePoints(clip.fadeOutCurve ?? "linear", "out", fadeOutPx, height)}
            />
          )}
        </svg>
      )}
      <EnvelopeLayer
        clip={clip}
        shown={shown}
        left={left}
        width={width}
        height={height}
        pxPerFrame={pxPerFrame}
        selected={selected}
      />
      {renaming ? (
        <span className="clip-label">
          <InlineName
            value={name}
            label={t("clip.rename", { name })}
            onCommit={(text) => nameClip(clip.id, text)}
            onDone={() => rename(null)}
          />
        </span>
      ) : (
        <span className="clip-label" ref={labelRef}>
          {name}
          {offline && <span className="clip-offline">{t("media.state.offline")}</span>}
          {bpm !== undefined && <span className="clip-bpm">{bpm} BPM</span>}
          {clip.gainDb !== 0 && <span className="clip-gain">{clip.gainDb > 0 ? "+" : ""}{clip.gainDb} dB</span>}
        </span>
      )}
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
