import { useFileDrop } from "../state/fileDrop";
import { addOrder } from "../voice/store";
import { memo, useMemo } from "react";
import type { MouseEvent } from "react";
import { useT } from "../i18n";
import type { Track } from "../project/types";
import { addCueEvent } from "../state/cues";
import { activeTimeline } from "../state/doc";
import { snapFrame, snapTargets } from "../state/snap";
import { useDoc } from "../state/store";
import { isSelected, menuAtPointer, useUi } from "../state/ui";
import { trackIds } from "../state/selection";
import { addNoteAt } from "../state/workflows";
import { clipLength, videoClipLength } from "../timeline/clip";
import { traceStyle } from "../timeline/palette";
import { frameX } from "../timeline/scale";
import { useVisibleRange } from "../timeline/useVisibleRange";
import { emptyMenu } from "./menus";
import { laneHeight } from "./TrackHeader";
import { ClipBlock } from "./AudioBlock";
import { VideoBlock } from "./VideoBlock";
import { NoteBlock, EventBlock, SceneBlock, OrderBlock } from "./EventBlocks";

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
