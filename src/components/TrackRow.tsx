import { memo, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import { useT } from "../i18n";
import type { AudioClip, Track, Trigger } from "../project/types";
import { moveTo, muteTrack, setGain } from "../state/actions";
import { useDoc } from "../state/store";
import { select, useUi } from "../state/ui";
import type { Selected } from "../state/ui";
import { AUDIO_LANE_H, EVENTS_LANE_H } from "../timeline/layout";
import { traceStyle } from "../timeline/palette";
import { frameX } from "../timeline/scale";
import { previewTrackGain } from "../transport/playback";
import { ClipWave } from "./ClipWave";
import { Fader } from "./Fader";
import { LevelMeter } from "./LevelMeter";
import { FlagIcon, MuteIcon, WaveIcon } from "./icons";

/** Length shown for a clip whose audio length is not known yet (10 s at the project rate). */
const GUESS_SECONDS = 10;

export function laneHeight(track: Track): number {
  return track.kind.type === "audio" ? AUDIO_LANE_H : EVENTS_LANE_H;
}

export const TrackHeader = memo(function TrackHeader({ track }: { track: Track }) {
  const t = useT();
  const audio = track.kind.type === "audio";
  return (
    <div
      className="track-header"
      style={{ height: laneHeight(track), ...traceStyle(track.color) }}
      data-muted={track.muted}
      data-audio={audio}
      data-colored={track.color !== undefined}
    >
      <div className="track-head-row">
        <span className="track-kind" title={t(audio ? "track.kind.audio" : "track.kind.events")}>
          {audio ? <WaveIcon /> : <FlagIcon />}
          <span className="visually-hidden">{t(audio ? "track.kind.audio" : "track.kind.events")}</span>
        </span>
        <span className="track-name" title={track.name}>
          {track.name}
        </span>
        {track.muted && <span className="track-muted-tag">{t("track.muted")}</span>}
        <button
          type="button"
          className="icon-button"
          aria-pressed={track.muted}
          aria-label={t("track.mute", { name: track.name })}
          title={t("track.mute", { name: track.name })}
          onClick={() => muteTrack(track.id)}
        >
          <MuteIcon on={track.muted} />
        </button>
      </div>
      {audio && (
        <>
          <Fader
            name={track.name}
            valueDb={track.gainDb ?? 0}
            onPreview={(db) => previewTrackGain(track.id, db)}
            onCommit={(db) => setGain(track.id, db)}
          />
          <LevelMeter trackId={track.id} name={track.name} />
        </>
      )}
    </div>
  );
});

/** Drag an item sideways in whole frames; commit once on release so undo takes one step. */
function useDrag(id: string, kind: Selected["kind"], position: number, pxPerFrame: number) {
  const [delta, setDelta] = useState(0);
  const origin = useRef<{ x: number; position: number } | null>(null);

  const handlers = {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      if (event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      origin.current = { x: event.clientX, position };
      select({ kind, id });
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      if (!origin.current) return;
      const frames = Math.round((event.clientX - origin.current.x) / pxPerFrame);
      setDelta(Math.max(frames, -origin.current.position));
    },
    onPointerUp() {
      if (origin.current && delta !== 0) moveTo(id, origin.current.position + delta);
      origin.current = null;
      setDelta(0);
    },
    onKeyDown(event: KeyboardEvent<HTMLElement>) {
      const step = event.shiftKey ? 25 : 1;
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        event.stopPropagation();
        moveTo(id, Math.max(0, position + (event.key === "ArrowLeft" ? -step : step)));
      }
    },
  };
  return { shown: position + delta, handlers };
}

/** Length in frames of a clip whose audio has been read, else the guess. */
export function clipLength(audio: { seconds: number } | undefined, fps: number, guess: number): number {
  return audio ? Math.round(audio.seconds * fps) : guess;
}

function ClipBlock({
  clip,
  name,
  guessLength,
  muted,
  rowHeight,
  fps,
  tint,
  pxPerFrame,
}: {
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
  const { shown, handlers: pointer } = useDrag(clip.id, "clip", clip.start, pxPerFrame);
  // Each clip asks the stores for its own slice: a selection or a decoded waveform elsewhere
  // does not re-render it.
  const selected = useUi((ui) => ui.selection?.id === clip.id);
  const audio = useDoc((doc) => doc.audio[clip.asset]);
  const width = Math.max(24, clipLength(audio, fps, guessLength) * pxPerFrame);
  return (
    <div
      className="clip"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-muted={muted}
      style={{ left: frameX(shown, pxPerFrame), width, height: rowHeight - 12 }}
      {...pointer}
    >
      <ClipWave
        seed={clip.asset}
        width={width}
        height={rowHeight - 12}
        color="currentColor"
        peaks={audio?.peaks}
        pxPerSecond={pxPerFrame * fps}
        tint={tint}
      />
      <span className="clip-label">
        {name}
        {clip.gainDb !== 0 && <span className="clip-gain">{clip.gainDb > 0 ? "+" : ""}{clip.gainDb} dB</span>}
      </span>
    </div>
  );
}

function TriggerFlag({ trigger, muted, pxPerFrame }: { trigger: Trigger; muted: boolean; pxPerFrame: number }) {
  const { shown, handlers: pointer } = useDrag(trigger.id, "trigger", trigger.at, pxPerFrame);
  const selected = useUi((ui) => ui.selection?.id === trigger.id);
  return (
    <div
      className="flag"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-muted={muted}
      data-kind={trigger.action.type}
      style={{ left: frameX(shown, pxPerFrame) }}
      {...pointer}
    >
      <span className="flag-chip">{trigger.label}</span>
    </div>
  );
}

export const TrackLane = memo(function TrackLane({
  track,
  assetNames,
  guessLength,
  fps,
  pxPerFrame,
}: {
  track: Track;
  assetNames: Record<string, string>;
  guessLength: number;
  fps: number;
  pxPerFrame: number;
}) {
  const height = laneHeight(track);
  return (
    <div className="lane" style={{ height, ...traceStyle(track.color) }} data-muted={track.muted} data-kind={track.kind.type}>
      {track.kind.type === "audio"
        ? track.kind.clips.map((clip) => (
            <ClipBlock
              key={clip.id}
              clip={clip}
              name={assetNames[clip.asset] ?? clip.asset}
              guessLength={guessLength}
              muted={track.muted}
              rowHeight={height}
              fps={fps}
              tint={`${track.color ?? ""}${track.muted}`}
              pxPerFrame={pxPerFrame}
            />
          ))
        : track.kind.triggers.map((trigger) => (
            <TriggerFlag key={trigger.id} trigger={trigger} muted={track.muted} pxPerFrame={pxPerFrame} />
          ))}
    </div>
  );
});

export { GUESS_SECONDS };
