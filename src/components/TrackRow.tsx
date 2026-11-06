import { memo, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, MouseEvent, PointerEvent } from "react";
import { useT } from "../i18n";
import type { ClipOp } from "../project/api";
import type { AudioClip, Note, Track } from "../project/types";
import { moveTo, muteTrack, setGain } from "../state/actions";
import { editClip } from "../state/clips";
import { activeTimeline } from "../state/doc";
import { snapFrame } from "../state/snap";
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
      // With the magnet on, the item's start lands on the closest grid line when it is near one.
      const target = Math.max(0, snapFrame(origin.current.position + frames));
      setDelta(target - origin.current.position);
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

/**
 * Length in frames of a clip: the trim the backend wrote, else the whole audio once it has been
 * read, else the guess. (The backend always writes a length when it moves the start.)
 */
export function clipLength(
  clip: AudioClip,
  audio: { seconds: number } | undefined,
  fps: number,
  guess: number,
): number {
  return clip.length ?? (audio ? Math.round(audio.seconds * fps) : guess);
}

type HandleProps = ReturnType<typeof useHandle>["handlers"];

/**
 * Drag one handle of a clip to a frame of the timeline. Only the frame is decided here: the backend
 * works out what it means (a trim, a fade) and its answer replaces the clip, so a drag is one step
 * of undo. The handle shows where it was dropped until the answer arrives.
 */
function useHandle(id: string, op: ClipOp, frame: number, pxPerFrame: number) {
  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null);
  const originX = useRef<number | null>(null);
  const at = drag?.from === frame ? drag.to : frame;

  const handlers = {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      if (event.button !== 0) return;
      event.stopPropagation(); // The clip underneath would start moving.
      event.currentTarget.setPointerCapture(event.pointerId);
      originX.current = event.clientX;
      select({ kind: "clip", id });
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      if (originX.current === null) return;
      const frames = Math.round((event.clientX - originX.current) / pxPerFrame);
      setDrag({ from: frame, to: Math.max(0, snapFrame(frame + frames)) });
    },
    onPointerUp() {
      originX.current = null;
      if (drag && drag.to !== frame) void editClip(id, op, drag.to).finally(() => setDrag(null));
      else setDrag(null);
    },
    onKeyDown(event: KeyboardEvent<HTMLElement>) {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      event.preventDefault();
      event.stopPropagation();
      const step = (event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? 25 : 1);
      void editClip(id, op, Math.max(0, frame + step));
    },
  };
  return { at, handlers };
}

/** A grip on a clip: its edges trim it, the squares at the top corners set its fades. */
function ClipHandle({
  edge,
  label,
  style,
  handlers,
}: {
  edge: "start" | "end" | "fadeIn" | "fadeOut";
  label: string;
  style?: CSSProperties;
  handlers: HandleProps;
}) {
  return (
    <span
      className="clip-handle"
      data-edge={edge}
      role="button"
      tabIndex={0}
      aria-label={label}
      title={label}
      style={style}
      {...handlers}
    />
  );
}

/** Pixels the fade handle is wide; its centre sits where the fade ends. */
const FADE_HANDLE_PX = 8;

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
  const bpm = useDoc((doc) => doc.project.assets.find((asset) => asset.id === clip.asset)?.bpm);
  const length = clipLength(clip, audio, fps, guessLength);
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
      data-muted={muted}
      style={{ left: frameX(left, pxPerFrame), width, height }}
      onContextMenu={(event) => {
        select({ kind: "clip", id: clip.id });
        menuAtPointer(event, audioMenu(t, track, clip));
      }}
      {...pointer}
    >
      <span className="clip-wave-shift" style={{ left: (clip.start - trimStart.at) * pxPerFrame }}>
        <ClipWave
          seed={clip.asset}
          width={waveWidth}
          height={height}
          color="currentColor"
          peaks={audio?.peaks}
          pxPerSecond={pxPerFrame * fps}
          startSeconds={(clip.offset ?? 0) / fps}
          tint={tint}
        />
      </span>
      {(fadeInPx > 0 || fadeOutPx > 0) && (
        <svg className="clip-fades" width={width} height={height} aria-hidden="true">
          {fadeInPx > 0 && <polyline points={`0,${height} ${fadeInPx},0`} />}
          {fadeOutPx > 0 && <polyline points={`${width - fadeOutPx},0 ${width},${height}`} />}
        </svg>
      )}
      <span className="clip-label">
        {name}
        {bpm !== undefined && <span className="clip-bpm">{bpm} BPM</span>}
        {clip.gainDb !== 0 && <span className="clip-gain">{clip.gainDb > 0 ? "+" : ""}{clip.gainDb} dB</span>}
      </span>
      <ClipHandle edge="start" label={t("clip.trimStart", { name })} handlers={trimStart.handlers} />
      <ClipHandle edge="end" label={t("clip.trimEnd", { name })} handlers={trimEnd.handlers} />
      <ClipHandle
        edge="fadeIn"
        label={t("clip.fadeIn", { name })}
        style={{ left: fadeInPx - FADE_HANDLE_PX / 2 }}
        handlers={fadeIn.handlers}
      />
      <ClipHandle
        edge="fadeOut"
        label={t("clip.fadeOut", { name })}
        style={{ left: width - fadeOutPx - FADE_HANDLE_PX / 2 }}
        handlers={fadeOut.handlers}
      />
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
    addNoteAt(t, start + snapFrame(Math.round(event.nativeEvent.offsetX / pxPerFrame)), track.id);
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
