import { memo, useRef, useState } from "react";
import type { KeyboardEvent, PointerEvent } from "react";
import { useT } from "../i18n";
import type { AudioClip, Track, Trigger } from "../project/types";
import { AUDIO_LANE_H, EVENTS_LANE_H } from "../timeline/layout";
import { previewTrackGain } from "../transport/playback";
import { ClipWave } from "./ClipWave";
import { Fader } from "./Fader";
import { LevelMeter } from "./LevelMeter";
import type { Peaks } from "./ClipWave";
import { FlagIcon, MuteIcon, WaveIcon } from "./icons";

/** Length shown for a clip whose audio length is not known yet (10 s at the project rate). */
const GUESS_SECONDS = 10;

export function laneHeight(track: Track): number {
  return track.kind.type === "audio" ? AUDIO_LANE_H : EVENTS_LANE_H;
}

export const TrackHeader = memo(function TrackHeader({
  track,
  onToggleMute,
  onGain,
}: {
  track: Track;
  onToggleMute: (trackId: string) => void;
  onGain: (trackId: string, gainDb: number) => void;
}) {
  const t = useT();
  const audio = track.kind.type === "audio";
  return (
    <div className="track-header" style={{ height: laneHeight(track) }} data-muted={track.muted} data-audio={audio}>
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
          onClick={() => onToggleMute(track.id)}
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
            onCommit={(db) => onGain(track.id, db)}
          />
          <LevelMeter trackId={track.id} name={track.name} />
        </>
      )}
    </div>
  );
});

export interface Selected {
  kind: "clip" | "trigger";
  id: string;
}

interface ItemHandlers {
  pxPerFrame: number;
  selected: Selected | null;
  onSelect: (selection: Selected) => void;
  onMove: (id: string, position: number) => void;
}

/** Drag an item sideways in whole frames; commit once on release so undo takes one step. */
function useDrag(
  id: string,
  kind: Selected["kind"],
  position: number,
  { pxPerFrame, onSelect, onMove }: ItemHandlers,
) {
  const [delta, setDelta] = useState(0);
  const origin = useRef<{ x: number; position: number } | null>(null);

  const handlers = {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      if (event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      origin.current = { x: event.clientX, position };
      onSelect({ kind, id });
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      if (!origin.current) return;
      const frames = Math.round((event.clientX - origin.current.x) / pxPerFrame);
      setDelta(Math.max(frames, -origin.current.position));
    },
    onPointerUp() {
      if (origin.current && delta !== 0) onMove(id, origin.current.position + delta);
      origin.current = null;
      setDelta(0);
    },
    onKeyDown(event: KeyboardEvent<HTMLElement>) {
      const step = event.shiftKey ? 25 : 1;
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        event.stopPropagation();
        onMove(id, Math.max(0, position + (event.key === "ArrowLeft" ? -step : step)));
      }
    },
  };
  return { shown: position + delta, handlers };
}

function ClipBlock({
  clip,
  name,
  length,
  muted,
  rowHeight,
  peaks,
  fps,
  ...handlers
}: {
  clip: AudioClip;
  name: string;
  length: number;
  muted: boolean;
  rowHeight: number;
  peaks?: Peaks;
  /** Real frames per second, to turn frames on screen into seconds of audio. */
  fps: number;
} & ItemHandlers) {
  const { shown, handlers: pointer } = useDrag(clip.id, "clip", clip.start, handlers);
  const selected = handlers.selected?.id === clip.id;
  const width = Math.max(24, length * handlers.pxPerFrame);
  return (
    <div
      className="clip"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-muted={muted}
      style={{ left: shown * handlers.pxPerFrame, width, height: rowHeight - 12 }}
      {...pointer}
    >
      <ClipWave
        seed={clip.asset}
        width={width}
        height={rowHeight - 12}
        color="currentColor"
        peaks={peaks}
        pxPerSecond={handlers.pxPerFrame * fps}
      />
      <span className="clip-label">
        {name}
        {clip.gainDb !== 0 && <span className="clip-gain">{clip.gainDb > 0 ? "+" : ""}{clip.gainDb} dB</span>}
      </span>
    </div>
  );
}

function TriggerFlag({
  trigger,
  muted,
  ...handlers
}: { trigger: Trigger; muted: boolean } & ItemHandlers) {
  const { shown, handlers: pointer } = useDrag(trigger.id, "trigger", trigger.at, handlers);
  const selected = handlers.selected?.id === trigger.id;
  return (
    <div
      className="flag"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-muted={muted}
      data-kind={trigger.action.type}
      style={{ left: shown * handlers.pxPerFrame }}
      {...pointer}
    >
      <span className="flag-chip">{trigger.label}</span>
    </div>
  );
}

export const TrackLane = memo(function TrackLane({
  track,
  assetNames,
  durations,
  waveforms,
  guessLength,
  fps,
  ...handlers
}: {
  track: Track;
  assetNames: Record<string, string>;
  durations: Record<string, number>;
  waveforms: Record<string, Peaks>;
  guessLength: number;
  fps: number;
} & ItemHandlers) {
  const height = laneHeight(track);
  return (
    <div className="lane" style={{ height }} data-muted={track.muted} data-kind={track.kind.type}>
      {track.kind.type === "audio"
        ? track.kind.clips.map((clip) => (
            <ClipBlock
              key={clip.id}
              clip={clip}
              name={assetNames[clip.asset] ?? clip.asset}
              length={durations[clip.asset] ?? guessLength}
              muted={track.muted}
              rowHeight={height}
              peaks={waveforms[clip.asset]}
              fps={fps}
              {...handlers}
            />
          ))
        : track.kind.triggers.map((trigger) => (
            <TriggerFlag key={trigger.id} trigger={trigger} muted={track.muted} {...handlers} />
          ))}
    </div>
  );
});

export { GUESS_SECONDS };
