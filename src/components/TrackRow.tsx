import { useAudioOutputName } from "../audio/routing";
import { usePreferences } from "../preferences/store";
import { setCollapsed } from "../state/ui";
import { useFileDrop } from "../state/fileDrop";
import { addOrder, operatorEdit, useVoice } from "../voice/store";
import type { VoiceOrder } from "../project/types";
import { memo, useMemo, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, MouseEvent, PointerEvent } from "react";
import { useT } from "../i18n";
import type { ClipOp } from "../project/api";
import type { AudioClip, DmxScene, MidiEvent, Note, OscEvent, Track, VideoClip } from "../project/types";
import { moveTo, muteTrack, nameClip, nameTrack, setGain } from "../state/actions";
import { addCueEvent, moveCueEvent } from "../state/cues";
import { MediaProgressBar } from "../media/progress";
import { focusMedia, useMedia } from "../media/store";
import { editClip, editVideoClip } from "../state/clips";
import { activeTimeline } from "../state/doc";
import { snapBlockStart, snapFrame, snapTargets } from "../state/snap";
import { useDoc } from "../state/store";
import { isSelected, menuAtPointer, rename, select, setSnapGuide, useUi } from "../state/ui";
import { moveItems } from "../state/items";
import { groupStart, pickedIds, selectRange, setGroupDelta, toggleItem, trackIds, useGroupDrag } from "../state/selection";
import { addNoteAt } from "../state/workflows";
import type { Selected } from "../state/ui";
import { AUDIO_LANE_H, NOTES_LANE_H } from "../timeline/layout";
import { clipLength, videoClipLength } from "../timeline/clip";
import type { VideoStrip } from "../video/api";
import { fadePoints } from "../audio/fade";
import { traceStyle } from "../timeline/palette";
import type { SnapTarget } from "../timeline/snap";
import { frameX } from "../timeline/scale";
import { useVisibleRange } from "../timeline/useVisibleRange";
import { previewTrackGain } from "../transport/playback";
import { ClipWave } from "./ClipWave";
import { EnvelopeLayer, addEnvelopePoint } from "./EnvelopeLayer";
import { Fader } from "./Fader";
import { InlineName } from "./InlineName";
import { LevelMeter } from "./LevelMeter";
import { Marquee } from "./Marquee";
import { ChevronRightIcon, DmxIcon, MidiIcon, MuteIcon, NoteIcon, OscIcon, OutputIcon, VideoIcon, WaveIcon, VoiceIcon } from "./icons";
import { audioMenu, cueTrackMenu, emptyMenu, eventMenu, noteMenu, notesTrackMenu, operatorOrderMenu, videoTrackMenu } from "./menus";

export function laneHeight(track: Track): number {
  return track.kind.type === "audio" || track.kind.type === "operator" || track.kind.type === "video" ? AUDIO_LANE_H : NOTES_LANE_H;
}

const KIND_ICONS = {
  audio: <WaveIcon />,
  operator: <VoiceIcon />,
  notes: <NoteIcon />,
  midi: <MidiIcon />,
  osc: <OscIcon />,
  dmx: <DmxIcon />,
  video: <VideoIcon />,
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
  const audio = track.kind.type === "audio" || track.kind.type === "operator" || track.kind.type === "video";
  const outputName = useAudioOutputName();
  const output = usePreferences((p) => p.output.trackRoutes?.find((r) => r.trackId === track.id)?.output);
  const selected = useUi((ui) => ui.selection?.kind === "track" && ui.selection.id === track.id);
  const renaming = useUi((ui) => ui.renaming?.kind === "track" && ui.renaming.id === track.id);
  return (
    <div
      className="track-header"
      style={{ height: laneHeight(track), ...traceStyle(track.color) }}
      data-selected={selected}
      onClick={() => select({ kind: "track", id: track.id })}
      data-muted={track.muted}
      data-audio={audio}
      data-colored={track.color !== undefined}
      data-dragging={dragging}
      onContextMenu={(event) =>
        menuAtPointer(
          event,
          audio
            ? audioMenu(t, track)
            : track.kind.type === "notes"
              ? notesTrackMenu(t, track)
              : track.kind.type === "video"
                ? videoTrackMenu(t, track)
                : cueTrackMenu(t, track),
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
          <Marquee
            className="track-name"
            title={track.name}
            onDoubleClick={() => rename({ kind: "track", id: track.id })}
          >
            {track.name}
          </Marquee>
        )}
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
      <div className="track-detail-row">
        {audio ? (
          <button
            type="button"
            className="track-output"
            aria-label={`${t("routing.trackOutput")}: ${outputName(output)}`}
            title={`${t("routing.trackOutput")}: ${outputName(output)}`}
            onClick={() => { select({ kind: "track", id: track.id }); setCollapsed(false); }}
          >
            <OutputIcon />
            <span className="track-output-name">{outputName(output)}</span>
            <ChevronRightIcon />
          </button>
        ) : (
          <span className="track-kind-label">{t(`track.kind.${track.kind.type}`)}</span>
        )}
        {track.muted && <span className="track-muted-tag">{t("track.muted")}</span>}
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
 * how many frames it lasts, so that its end can land on something as well as its start. When the item
 * is one of several selected, the whole group moves with it and the backend moves them in one step.
 */
function useDrag(id: string, kind: Selected["kind"], position: number, length: number, pxPerFrame: number, track: Track, onClick?: () => void) {
  // A clip or a note is moved by the window's own edit; a MIDI or OSC event by the backend.
  const move =
    kind === "operator"
      ? (id: string, at: number) => operatorEdit({ op: "moveOrder", id, at })
      : kind === "event"
        ? moveCueEvent
        : kind === "video"
          ? (id: string, at: number) => editVideoClip(id, "move", at)
          : moveTo;
  const [delta, setDelta] = useState(0);
  // The group's drag is shared, so every selected item follows the one held.
  const grouped = useUi((ui) => ui.picked.has(id));
  const groupDelta = useGroupDrag((drag) => (grouped ? drag.delta : 0));
  const origin = useRef<{ x: number; position: number; targets: SnapTarget[]; floor: number; group: boolean; click: "collapse" | "toggle" | null } | null>(null);

  // Moves the item by `by` frames; with a group or ripple on, the backend moves them.
  const commit = (by: number) => {
    const ui = useUi.getState();
    if (ui.picked.has(id) || (ui.ripple !== "off" && kind !== "operator")) void moveItems(ui.picked.has(id) ? pickedIds() : [id], by);
    else void move(id, Math.max(0, position + by));
  };

  const handlers = {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      if (event.button !== 0) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      const item = { kind, id } as const;
      let click: "collapse" | "toggle" | null = null;
      if (kind !== "operator" && event.shiftKey) {
        selectRange(track, item);
        origin.current = null;
        return;
      }
      if (kind !== "operator" && (event.ctrlKey || event.metaKey)) {
        if (isSelected(useUi.getState(), id)) click = "toggle";
        else toggleItem(item);
      } else if (kind !== "operator" && useUi.getState().picked.has(id)) click = "collapse";
      else select(item);
      const group = useUi.getState().picked.has(id);
      const ids = group ? pickedIds() : [id];
      origin.current = { x: event.clientX, position, targets: snapTargets(ids), floor: group ? groupStart(ids) : position, group, click };
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      const o = origin.current;
      if (!o) return;
      const frames = Math.round((event.clientX - o.x) / pxPerFrame);
      // With the magnet on, the item's start or end lands on the closest grid line, item or the
      // playhead when it is near one. The group stops where its first item reaches the start.
      const target = snapBlockStart(o.position + frames, length, o.targets, { alt: event.altKey });
      const by = Math.max(target - o.position, o.group ? -o.floor : -o.position);
      if (o.group) setGroupDelta(by);
      else setDelta(by);
    },
    onPointerUp() {
      const o = origin.current;
      origin.current = null;
      setSnapGuide(null);
      if (!o) return;
      const by = o.group ? useGroupDrag.getState().delta : delta;
      if (by !== 0) commit(by);
      else {
        // A press that never moved: a click.
        if (o.click === "collapse") select({ kind, id });
        else if (o.click === "toggle") toggleItem({ kind, id });
        onClick?.();
      }
      setGroupDelta(0);
      setDelta(0);
    },
    onKeyDown(event: KeyboardEvent<HTMLElement>) {
      const step = event.shiftKey ? 25 : 1;
      if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        event.stopPropagation();
        commit(event.key === "ArrowLeft" ? -step : step);
      }
    },
  };
  return { shown: position + (grouped ? groupDelta : delta), handlers };
}

type HandleProps = ReturnType<typeof useHandle>["handlers"];

/**
 * Drag one handle of a clip to a frame of the timeline. Only the frame is decided here: the backend
 * works out what it means (a trim, a fade) and its answer replaces the clip, so a drag is one step
 * of undo. The handle shows where it was dropped until the answer arrives.
 */
function useHandle(id: string, op: ClipOp, frame: number, pxPerFrame: number, video = false) {
  const edit = (to: number) =>
    video ? editVideoClip(id, op as "trimStart" | "trimEnd" | "fadeIn" | "fadeOut", to) : editClip(id, op, to);
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
      select({ kind: video ? "video" : "clip", id });
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      if (originX.current === null) return;
      const frames = Math.round((event.clientX - originX.current) / pxPerFrame);
      setDrag({ from: frame, to: Math.max(0, snapFrame(frame + frames, targets.current, { alt: event.altKey, guide: true })) });
    },
    onPointerUp() {
      originX.current = null;
      setSnapGuide(null);
      if (drag && drag.to !== frame) void edit(drag.to).finally(() => setDrag(null));
      else setDrag(null);
    },
    onKeyDown(event: KeyboardEvent<HTMLElement>) {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      event.preventDefault();
      event.stopPropagation();
      const step = (event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? 25 : 1);
      void edit(Math.max(0, frame + step));
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

/**
 * Pixels the fade handle is wide; its centre sits where the fade ends, but it never leaves the clip,
 * so with no fade it sits whole in the corner instead of half outside it. What takes the click is
 * larger than what is drawn (see `.clip-handle[data-edge="fadeIn"]::before`).
 */
const FADE_HANDLE_PX = 10;

/**
 * Where the fade handle starts, in pixels from the left of a clip `width` wide, for a fade ending at
 * `end`. Handles sit inside the clip's 1px border, so its room is two pixels less than `width`.
 */
const fadeHandleLeft = (end: number, width: number) =>
  Math.min(Math.max(end - FADE_HANDLE_PX / 2, 0), Math.max(0, width - 2 - FADE_HANDLE_PX));

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

/** A video clip: moved by dragging it, trimmed by its edges, faded by the squares at its top corners, cut with S at the playhead. The backend makes each edit. */
function VideoBlock({
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

/**
 * A note on its lane: a line at its start with the text beside it. One with a length is drawn as a
 * box as long as it lasts, so what is on show at a point of the timeline can be read off the ruler.
 */
function NoteBlock({ track, note, pxPerFrame }: { track: Track; note: Note; pxPerFrame: number }) {
  const t = useT();
  const { shown, handlers: pointer } = useDrag(note.id, "note", note.at, note.length ?? 0, pxPerFrame, track);
  const selected = useUi((ui) => isSelected(ui, note.id));
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
        if (!isSelected(useUi.getState(), note.id)) select({ kind: "note", id: note.id });
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
  const { shown, handlers: pointer } = useDrag(event.id, "event", event.at, 0, pxPerFrame, track);
  const selected = useUi((ui) => isSelected(ui, event.id));
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
        if (!isSelected(useUi.getState(), event.id)) select({ kind: "event", id: event.id });
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

/**
 * A DMX scene on its lane: a line at its time with its name, and a bar as long as its fade, so the
 * span the lights take to get there can be read off the ruler.
 */
function SceneBlock({ track, scene, pxPerFrame }: { track: Track; scene: DmxScene; pxPerFrame: number }) {
  const t = useT();
  const fade = scene.fade ?? 0;
  const { shown, handlers: pointer } = useDrag(scene.id, "event", scene.at, fade, pxPerFrame, track);
  const selected = useUi((ui) => isSelected(ui, scene.id));
  return (
    <div
      className="cue-event dmx-scene"
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      data-selected={selected}
      data-muted={track.muted}
      data-kind="dmx"
      style={{ left: frameX(shown, pxPerFrame) }}
      onContextMenu={(menu) => {
        if (!isSelected(useUi.getState(), scene.id)) select({ kind: "event", id: scene.id });
        menuAtPointer(menu, eventMenu(t, track, scene.id, scene.at));
      }}
      {...pointer}
    >
      {fade > 0 && <span className="dmx-fade" style={{ width: fade * pxPerFrame }} />}
      <span className="cue-event-body">
        <span className="cue-event-tag">{t("event.tag.dmx")}</span>
        {scene.name && <span className="cue-event-detail">{scene.name}</span>}
      </span>
    </div>
  );
}

function OrderBlock({ track, order, fps, pxPerFrame }: { track: Track; order: VoiceOrder; fps: number; pxPerFrame: number }) {
  const t = useT();
  const view = useVoice((v) => v.views[order.id]);
  const busy = useVoice((v) => v.busy === order.id);
  const audio = useDoc((s) => order.generated ? s.audio[order.generated.asset] : undefined);
  const width = view?.start != null && view.length ? Math.max(24, view.length * pxPerFrame) : undefined;
  const selected = useUi((ui) => ui.selection?.id === order.id);
  const { shown, handlers } = useDrag(order.id, "operator", order.at, 0, pxPerFrame, track);
  return <div className="operator-order" role="button" tabIndex={0} aria-pressed={selected}
    data-selected={selected} data-muted={track.muted} data-status={view?.status ?? "ungenerated"} data-range={view?.start != null}
    style={{ left: frameX(view?.start ?? order.at, pxPerFrame), transform: `translateX(${(shown - order.at) * pxPerFrame}px)`, width }}
    onContextMenu={(event) => { select({ kind: "operator", id: order.id }); menuAtPointer(event, operatorOrderMenu(t, track, order.id)); }}
    title={order.dialogue} onFocus={() => select({ kind: "operator", id: order.id })} {...handlers}>
    {order.generated && audio?.peaks && width !== undefined && view && <span className="operator-order-wave">
      <ClipWave seed={order.generated.asset} width={width} height={44}
        origin={frameX((view.start ?? order.at) + shown - order.at, pxPerFrame)}
        color="currentColor" peaks={audio.peaks} pxPerSecond={pxPerFrame * fps}
        startSeconds={view.audioOffsetSeconds} tint={`${track.color ?? ""}${track.muted}`} />
    </span>}
    <span className="operator-anchor" data-end={order.alignment === "end" && !view?.beforeOrigin} />
    <span className="operator-order-text">{order.dialogue || t("voice.empty")}</span>
    <span className="operator-order-state">{t(busy ? "voice.preparingRecording" : `voice.status.${view?.status ?? "ungenerated"}`)}{view?.beforeOrigin && ` · ${t("voice.positionInvalid")}`}</span>
  </div>;
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
  // Only what is near the screen is mounted: a long show has thousands of items, a canvas per clip.
  const range = useVisibleRange(Number.POSITIVE_INFINITY);
  const audioInfo = useDoc((doc) => doc.audio);
  const videoInfo = useDoc((doc) => doc.video);
  // The selected items stay mounted, so a drag that carries them out of the range is not cut short.
  // Only this lane's own selection counts (as one string, which compares by value), so a selection
  // elsewhere does not redraw it.
  const keptKey = useUi((ui) => {
    if (!ui.selection && ui.picked.size === 0) return "";
    return trackIds(track).filter((id) => isSelected(ui, id)).join("|");
  });
  const kept = useMemo(() => new Set(keptKey.split("|")), [keptKey]);
  const near = (id: string, start: number, length: number) =>
    kept.has(id) || (frameX(start + length, pxPerFrame) >= range.from && frameX(start, pxPerFrame) <= range.to);
  // Files from the file explorer are over this lane.
  const dropping = useFileDrop((drop) => drop.timeline?.track === track.id);
  // A double click on the empty part of a notes, MIDI or OSC lane adds one there.
  const addHere = (event: MouseEvent<HTMLDivElement>) => {
    const kind = track.kind.type;
    // Video clips are not drawn or added from the timeline yet.
    if (kind === "audio" || kind === "video" || event.target !== event.currentTarget) return;
    const { start } = activeTimeline(useDoc.getState()).timecode;
    const frame = start + snapFrame(Math.round(event.nativeEvent.offsetX / pxPerFrame), snapTargets());
    if (kind === "operator") void addOrder(track.id, frame - start);
    else if (kind === "notes") addNoteAt(t, frame, track.id);
    else void addCueEvent(kind, frame, t(`track.kind.${kind}`), track.id);
  };
  return (
    <div
      className="lane"
      style={{ height, ...traceStyle(track.color) }}
      data-drop-track={track.id}
      data-drop={dropping}
      data-muted={track.muted}
      data-kind={track.kind.type}
      data-dragging={dragging}
      onDoubleClick={addHere}
      onContextMenu={(event) => {
        const { start } = activeTimeline(useDoc.getState()).timecode;
        const frame = start + Math.max(0, Math.round((event.clientX - event.currentTarget.getBoundingClientRect().left) / pxPerFrame));
        menuAtPointer(
          event,
          emptyMenu(t, track.kind.type === "audio" || track.kind.type === "video" ? undefined : { frame, trackId: track.id, kind: track.kind.type }),
        );
      }}
    >
      {track.kind.type === "operator" && track.kind.orders.length === 0 && <button className="button operator-add" type="button" onClick={() => void addOrder(track.id, 0)}>{t("voice.addOrder")}</button>}
      {track.kind.type === "audio"
        ? track.kind.clips.filter((clip) => near(clip.id, clip.start, clipLength(clip, audioInfo[clip.asset], fps, guessLength))).map((clip) => (
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
          ? track.kind.notes.filter((note) => near(note.id, note.at, note.length ?? 0)).map((note) => (
              <NoteBlock key={note.id} track={track} note={note} pxPerFrame={pxPerFrame} />
            ))
          : track.kind.type === "operator"
            ? track.kind.orders.map((order) => <OrderBlock key={order.id} track={track} order={order} fps={fps} pxPerFrame={pxPerFrame} />)
          : track.kind.type === "video"
            ? track.kind.clips.filter((clip) => near(clip.id, clip.start, videoClipLength(clip, videoInfo[clip.asset], fps, guessLength))).map((clip) => (
                <VideoBlock key={clip.id} track={track} clip={clip} guessLength={guessLength} rowHeight={height} fps={fps} pxPerFrame={pxPerFrame} />
              ))
          : track.kind.type === "dmx"
            ? track.kind.scenes.filter((scene) => near(scene.id, scene.at, scene.fade ?? 0)).map((scene) => (
                <SceneBlock key={scene.id} track={track} scene={scene} pxPerFrame={pxPerFrame} />
              ))
          : track.kind.events.filter((event) => near(event.id, event.at, 0)).map((event) => (
              <EventBlock key={event.id} track={track} event={event} pxPerFrame={pxPerFrame} />
            ))}
    </div>
  );
});
