import { memo, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, MouseEvent, PointerEvent } from "react";
import { useT } from "../i18n";
import type { ClipOp } from "../project/api";
import type { AudioClip, MidiEvent, Note, OscEvent, Track } from "../project/types";
import { moveTo, muteTrack, nameClip, nameTrack, setGain } from "../state/actions";
import { addCueEvent, moveCueEvent } from "../state/cues";
import { editClip } from "../state/clips";
import { activeTimeline } from "../state/doc";
import { snapBlockStart, snapFrame, snapTargets } from "../state/snap";
import { useDoc } from "../state/store";
import { menuAtPointer, rename, select, setSnapGuide, useUi } from "../state/ui";
import { addNoteAt } from "../state/workflows";
import type { Selected } from "../state/ui";
import { AUDIO_LANE_H, NOTES_LANE_H } from "../timeline/layout";
import { clipLength } from "../timeline/clip";
import { fadePoints } from "../audio/fade";
import { traceStyle } from "../timeline/palette";
import type { SnapTarget } from "../timeline/snap";
import { frameX } from "../timeline/scale";
import { previewTrackGain } from "../transport/playback";
import { ClipWave } from "./ClipWave";
import { EnvelopeLayer } from "./EnvelopeLayer";
import { Fader } from "./Fader";
import { InlineName } from "./InlineName";
import { LevelMeter } from "./LevelMeter";
import { MidiIcon, MuteIcon, NoteIcon, OscIcon, WaveIcon } from "./icons";
import { audioMenu, cueTrackMenu, emptyMenu, eventMenu, noteMenu, notesTrackMenu } from "./menus";

export function laneHeight(track: Track): number {
  return track.kind.type === "audio" ? AUDIO_LANE_H : NOTES_LANE_H;
}

const KIND_ICONS = {
  audio: <WaveIcon />,
  notes: <NoteIcon />,
  midi: <MidiIcon />,
  osc: <OscIcon />,
} as const;

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
  const renaming = useUi((ui) => ui.renaming?.kind === "track" && ui.renaming.id === track.id);
  return (
    <div
      className="track-header"
      style={{ height: laneHeight(track), ...traceStyle(track.color) }}
      data-muted={track.muted}
      data-audio={audio}
      data-colored={track.color !== undefined}
      data-dragging={dragging}
      onContextMenu={(event) =>
        menuAtPointer(
          event,
          audio ? audioMenu(t, track) : track.kind.type === "notes" ? notesTrackMenu(t, track) : cueTrackMenu(t, track),
        )
      }
    >
      <div className="track-head-row" onPointerDown={(event) => onGrab(event, track.id)}>
        <span className="track-kind" title={t(`track.kind.${track.kind.type}`)}>
          {KIND_ICONS[track.kind.type]}
          <span className="visually-hidden">{t(`track.kind.${track.kind.type}`)}</span>
        </span>
        {renaming ? (
          <InlineName
            value={track.name}
            label={t("track.rename", { name: track.name })}
            onCommit={(text) => nameTrack(track.id, text)}
            onDone={() => rename(null)}
          />
        ) : (
          <span
            className="track-name"
            title={track.name}
            onDoubleClick={() => rename({ kind: "track", id: track.id })}
          >
            {track.name}
          </span>
        )}
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

/**
 * Drag an item sideways in whole frames; commit once on release so undo takes one step. `length` is
 * how many frames it lasts, so that its end can land on something as well as its start.
 */
function useDrag(id: string, kind: Selected["kind"], position: number, length: number, pxPerFrame: number) {
  // A clip or a note is moved by the window's own edit; a MIDI or OSC event by the backend.
  const move = kind === "event" ? moveCueEvent : moveTo;
  const [delta, setDelta] = useState(0);
  const origin = useRef<{ x: number; position: number; targets: SnapTarget[] } | null>(null);

  const handlers = {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      if (event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      origin.current = { x: event.clientX, position, targets: snapTargets([id]) };
      select({ kind, id });
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      if (!origin.current) return;
      const frames = Math.round((event.clientX - origin.current.x) / pxPerFrame);
      // With the magnet on, the item's start or end lands on the closest grid line, item or the
      // playhead when it is near one.
      const target = Math.max(0, snapBlockStart(origin.current.position + frames, length, origin.current.targets, { alt: event.altKey }));
      setDelta(target - origin.current.position);
    },
    onPointerUp() {
      if (origin.current && delta !== 0) void move(id, origin.current.position + delta);
      origin.current = null;
      setSnapGuide(null);
      setDelta(0);
    },
    onKeyDown(event: KeyboardEvent<HTMLElement>) {
      const step = event.shiftKey ? 25 : 1;
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        event.stopPropagation();
        void move(id, Math.max(0, position + (event.key === "ArrowLeft" ? -step : step)));
      }
    },
  };
  return { shown: position + delta, handlers };
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
  const targets = useRef<SnapTarget[]>([]);
  const at = drag?.from === frame ? drag.to : frame;

  const handlers = {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      if (event.button !== 0) return;
      event.stopPropagation(); // The clip underneath would start moving.
      event.currentTarget.setPointerCapture(event.pointerId);
      originX.current = event.clientX;
      targets.current = snapTargets([id]);
      select({ kind: "clip", id });
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      if (originX.current === null) return;
      const frames = Math.round((event.clientX - originX.current) / pxPerFrame);
      setDrag({ from: frame, to: Math.max(0, snapFrame(frame + frames, targets.current, { alt: event.altKey, guide: true })) });
    },
    onPointerUp() {
      originX.current = null;
      setSnapGuide(null);
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
  // Each clip asks the stores for its own slice: a selection or a decoded waveform elsewhere
  // does not re-render it.
  const selected = useUi((ui) => ui.selection?.id === clip.id);
  const renaming = useUi((ui) => ui.renaming?.kind === "clip" && ui.renaming.id === clip.id);
  const audio = useDoc((doc) => doc.audio[clip.asset]);
  const bpm = useDoc((doc) => doc.project.assets.find((asset) => asset.id === clip.asset)?.bpm);
  const length = clipLength(clip, audio, fps, guessLength);
  const { shown, handlers: pointer } = useDrag(clip.id, "clip", clip.start, length, pxPerFrame);
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
        <span className="clip-label" onDoubleClick={() => rename({ kind: "clip", id: clip.id })}>
          {name}
          {bpm !== undefined && <span className="clip-bpm">{bpm} BPM</span>}
          {clip.gainDb !== 0 && <span className="clip-gain">{clip.gainDb > 0 ? "+" : ""}{clip.gainDb} dB</span>}
        </span>
      )}
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
  const { shown, handlers: pointer } = useDrag(note.id, "note", note.at, note.length ?? 0, pxPerFrame);
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

/**
 * A MIDI or OSC event on its lane: a line at its frame with a tag naming what it sends (an OSC
 * event also shows its address). MIDI tags are solid and OSC tags dashed, so the kind never rides
 * on colour alone.
 */
function EventBlock({ track, event, pxPerFrame }: { track: Track; event: MidiEvent | OscEvent; pxPerFrame: number }) {
  const t = useT();
  const { shown, handlers: pointer } = useDrag(event.id, "event", event.at, 0, pxPerFrame);
  const selected = useUi((ui) => ui.selection?.id === event.id);
  const osc = "address" in event;
  const tag = t(osc ? "event.tag.osc" : (`event.tag.${event.message.type}` as const));
  return (
    <div
      className="cue-event"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-muted={track.muted}
      data-kind={osc ? "osc" : "midi"}
      style={{ left: frameX(shown, pxPerFrame) }}
      onContextMenu={(menu) => {
        select({ kind: "event", id: event.id });
        menuAtPointer(menu, eventMenu(t, track, event.id, event.at));
      }}
      {...pointer}
    >
      <span className="cue-event-body">
        <span className="cue-event-tag">{tag}</span>
        {osc && <span className="cue-event-detail">{event.address}</span>}
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
  // A double click on the empty part of a notes, MIDI or OSC lane adds one there.
  const addHere = (event: MouseEvent<HTMLDivElement>) => {
    const kind = track.kind.type;
    if (kind === "audio" || event.target !== event.currentTarget) return;
    const { start } = activeTimeline(useDoc.getState()).timecode;
    const frame = start + snapFrame(Math.round(event.nativeEvent.offsetX / pxPerFrame), snapTargets());
    if (kind === "notes") addNoteAt(t, frame, track.id);
    else void addCueEvent(kind, frame, t(`track.kind.${kind}`), track.id);
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
        menuAtPointer(
          event,
          emptyMenu(t, track.kind.type === "audio" ? undefined : { frame, trackId: track.id, kind: track.kind.type }),
        );
      }}
    >
      {track.kind.type === "audio"
        ? track.kind.clips.map((clip) => (
            <ClipBlock
              key={clip.id}
              track={track}
              clip={clip}
              name={clip.name ?? assetNames[clip.asset] ?? clip.asset}
              guessLength={guessLength}
              muted={track.muted}
              rowHeight={height}
              fps={fps}
              tint={`${track.color ?? ""}${track.muted}`}
              pxPerFrame={pxPerFrame}
            />
          ))
        : track.kind.type === "notes"
          ? track.kind.notes.map((note) => (
              <NoteBlock key={note.id} track={track} note={note} pxPerFrame={pxPerFrame} />
            ))
          : track.kind.events.map((event) => (
              <EventBlock key={event.id} track={track} event={event} pxPerFrame={pxPerFrame} />
            ))}
    </div>
  );
});
