import { memo, useRef, useState } from "react";
import type { KeyboardEvent, MouseEvent, PointerEvent } from "react";
import { useT } from "../i18n";
import type { AudioClip, Note, Track } from "../project/types";
import { moveTo, muteTrack, setGain } from "../state/actions";
import { activeTimeline } from "../state/doc";
import { useDoc } from "../state/store";
import { menuAtPointer, select, useUi } from "../state/ui";
import { addNoteAt } from "../state/workflows";
import type { Selected } from "../state/ui";
import { AUDIO_LANE_H, NOTES_LANE_H } from "../timeline/layout";
import { traceStyle } from "../timeline/palette";
import { frameX } from "../timeline/scale";
import { previewTrackGain } from "../transport/playback";
import { ClipWave } from "./ClipWave";
import { Fader } from "./Fader";
import { LevelMeter } from "./LevelMeter";
import { MuteIcon, NoteIcon, WaveIcon } from "./icons";
import { audioMenu, emptyMenu, noteMenu, notesTrackMenu } from "./menus";

/** Length shown for a clip whose audio length is not known yet (10 s at the project rate). */
const GUESS_SECONDS = 10;

export function laneHeight(track: Track): number {
  return track.kind.type === "audio" ? AUDIO_LANE_H : NOTES_LANE_H;
}

export const TrackHeader = memo(function TrackHeader({
  track,
  dragging,
  onGrab,
}: {
  track: Track;
  dragging: boolean;
  /** Starts dragging the track to another place in the order. */
  onGrab: (event: PointerEvent<HTMLElement>, id: string) => void;
}) {
  const t = useT();
  const audio = track.kind.type === "audio";
  return (
    <div
      className="track-header"
      style={{ height: laneHeight(track), ...traceStyle(track.color) }}
      data-muted={track.muted}
      data-audio={audio}
      data-colored={track.color !== undefined}
      data-dragging={dragging}
      onContextMenu={(event) => menuAtPointer(event, audio ? audioMenu(t, track) : notesTrackMenu(t, track))}
    >
      <div className="track-head-row" onPointerDown={(event) => onGrab(event, track.id)}>
        <span className="track-kind" title={t(audio ? "track.kind.audio" : "track.kind.notes")}>
          {audio ? <WaveIcon /> : <NoteIcon />}
          <span className="visually-hidden">{t(audio ? "track.kind.audio" : "track.kind.notes")}</span>
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
      onContextMenu={(event) => {
        select({ kind: "clip", id: clip.id });
        menuAtPointer(event, audioMenu(t, track, clip));
      }}
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

/**
 * A note on its lane: a line at its start with the text beside it. One with a length is drawn as a
 * box as long as it lasts, so what is on show at a point of the timeline can be read off the ruler.
 */
function NoteBlock({ track, note, pxPerFrame }: { track: Track; note: Note; pxPerFrame: number }) {
  const t = useT();
  const { shown, handlers: pointer } = useDrag(note.id, "note", note.at, pxPerFrame);
  const selected = useUi((ui) => ui.selection?.id === note.id);
  const empty = note.text.trim() === "";
  const timed = note.length !== undefined;
  // A note with no length is a label at a point: its first line. One with a length is a box with room for more.
  const label = timed ? note.text : (note.text.split("\n").find((line) => line.trim() !== "") ?? "");
  return (
    <div
      className="note"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-muted={track.muted}
      data-timed={timed}
      style={{ left: frameX(shown, pxPerFrame) }}
      onContextMenu={(event) => {
        select({ kind: "note", id: note.id });
        menuAtPointer(event, noteMenu(t, track, note));
      }}
      {...pointer}
    >
      <span
        className="note-body"
        data-empty={empty}
        style={note.length === undefined ? undefined : { width: Math.max(24, note.length * pxPerFrame) }}
      >
        {empty ? t("note.empty") : label}
      </span>
    </div>
  );
}

export const TrackLane = memo(function TrackLane({
  track,
  assetNames,
  guessLength,
  fps,
  pxPerFrame,
  dragging,
}: {
  track: Track;
  assetNames: Record<string, string>;
  guessLength: number;
  fps: number;
  pxPerFrame: number;
  dragging: boolean;
}) {
  const t = useT();
  const height = laneHeight(track);
  // A double click on the empty part of a notes lane writes a note there.
  const addHere = (event: MouseEvent<HTMLDivElement>) => {
    if (track.kind.type !== "notes" || event.target !== event.currentTarget) return;
    const { start } = activeTimeline(useDoc.getState()).timecode;
    addNoteAt(t, start + Math.round(event.nativeEvent.offsetX / pxPerFrame), track.id);
  };
  return (
    <div
      className="lane"
      style={{ height, ...traceStyle(track.color) }}
      data-muted={track.muted}
      data-kind={track.kind.type}
      data-dragging={dragging}
      onDoubleClick={addHere}
      onContextMenu={(event) => {
        const { start } = activeTimeline(useDoc.getState()).timecode;
        const frame = start + Math.max(0, Math.round((event.clientX - event.currentTarget.getBoundingClientRect().left) / pxPerFrame));
        menuAtPointer(event, emptyMenu(t, track.kind.type === "notes" ? { frame, trackId: track.id } : undefined));
      }}
    >
      {track.kind.type === "audio"
        ? track.kind.clips.map((clip) => (
            <ClipBlock
              key={clip.id}
              track={track}
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
        : track.kind.notes.map((note) => (
            <NoteBlock key={note.id} track={track} note={note} pxPerFrame={pxPerFrame} />
          ))}
    </div>
  );
});

export { GUESS_SECONDS };
