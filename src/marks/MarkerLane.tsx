import { useRef, useState } from "react";
import type { CSSProperties, PointerEvent } from "react";
import { useT } from "../i18n";
import type { MessageKey, Translate } from "../i18n/messages";
import type { MarksOp } from "../project/api";
import type { Marker, Region, TempoChange } from "../project/types";
import { activeTimeline } from "../state/doc";
import { addTempoChangeAt, editMarks, renameMark, selectMark, useMarks } from "../state/marks";
import type { MarkRef } from "../state/marks";
import { snapBlockStart, snapFrame, snapTargets } from "../state/snap";
import { useDoc, useTimeline } from "../state/store";
import { menuAtPointer, select, setSnapGuide } from "../state/ui";
import type { MenuItem } from "../state/ui";
import { formatFrames } from "../timecode";
import { frameX } from "../timeline/scale";
import { TRACK_COLORS } from "../timeline/palette";
import type { SnapTarget } from "../timeline/snap";
import { InlineName } from "../components/InlineName";
import { TempoChangeEditor } from "./TempoChangeEditor";

/** Height of the lane of markers and regions, under the ruler. */
export const MARKER_RAIL_H = 22;

/** Pixels the pointer must move before a press becomes a drag. */
const DRAG_PX = 4;

type Mark = { kind: MarkRef["kind"]; id: string };

/**
 * Follows the pointer from a press: calls `move` with the frame under it (snapped, relative to the
 * timeline origin) once it has moved a few pixels, and `end` when it is let go. Escape cancels.
 */
function follow(
  event: PointerEvent,
  lane: HTMLElement,
  pxPerFrame: number,
  targets: SnapTarget[] | null,
  move: (frame: number) => void,
  end: (frame: number, moved: boolean) => void,
): void {
  const startX = event.clientX;
  let moved = false;
  let frame = 0;
  const at = (e: { clientX: number; altKey: boolean }) => {
    const raw = Math.max(0, Math.round((e.clientX - lane.getBoundingClientRect().left) / pxPerFrame));
    return targets === null ? raw : Math.max(0, snapFrame(raw, targets, { alt: e.altKey, guide: true }));
  };
  const finish = (commit: boolean) => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onCancel);
    window.removeEventListener("keydown", onKey, true);
    setSnapGuide(null);
    if (commit) end(frame, moved);
  };
  const onMove = (e: globalThis.PointerEvent) => {
    if (!moved && Math.abs(e.clientX - startX) < DRAG_PX) return;
    moved = true;
    frame = at(e);
    move(frame);
  };
  const onUp = (e: globalThis.PointerEvent) => {
    if (moved) frame = at(e);
    finish(true);
  };
  const onCancel = () => finish(false);
  const onKey = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    e.stopPropagation();
    finish(false);
  };
  frame = at(event.nativeEvent);
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onCancel);
  window.addEventListener("keydown", onKey, true);
}

/** The colour of a mark as the style the lane draws with; none keeps the neutral default. */
const tint = (color: string | undefined): CSSProperties | undefined => (color ? ({ "--mark": color } as CSSProperties) : undefined);

/** The rows that set the colour of a marker or a region. */
function colorMenu(t: Translate, current: string | undefined, set: (color: string | null) => void): MenuItem[] {
  return [
    { label: t("track.color.default"), checked: current === undefined, run: () => set(null) },
    ...TRACK_COLORS.map(({ id, hex }): MenuItem => ({
      label: t(`track.color.${id}` as MessageKey),
      checked: current === hex,
      run: () => set(hex),
    })),
  ];
}

/**
 * The lane under the ruler for the markers, the regions and the tempo changes. A marker is dragged
 * to move it, a region is drawn by dragging in the empty lane and resized by its edges, a double
 * click renames, and Delete removes what is picked. The backend keeps them in order and does the rest.
 */
export function MarkerLane({ pxPerFrame, width }: { pxPerFrame: number; width: number }) {
  const t = useT();
  const timeline = useTimeline();
  const { frameRate, start } = timeline.timecode;
  const { markers = [], regions = [], tempoChanges = [] } = timeline;
  const selected = useMarks((marks) => marks.selected);
  const renaming = useMarks((marks) => marks.renaming);
  const lane = useRef<HTMLDivElement>(null);
  // What is being dragged: where it would land now, shown until the backend answers.
  const [live, setLive] = useState<{ id: string; at: number; end?: number } | null>(null);
  const [drawing, setDrawing] = useState<{ start: number; end: number } | null>(null);

  const x = (frame: number) => frameX(frame, pxPerFrame);
  const isSelected = (kind: Mark["kind"], id: string) => selected?.kind === kind && selected.id === id;
  const pick = (kind: Mark["kind"], id: string) => {
    select(null);
    selectMark({ kind, id });
  };

  const grabMarker = (event: PointerEvent, marker: Marker) => {
    if (event.button !== 0 || !lane.current) return;
    event.stopPropagation();
    pick("marker", marker.id);
    follow(
      event,
      lane.current,
      pxPerFrame,
      snapTargets([marker.id], ["origin"]),
      (frame) => setLive({ id: marker.id, at: frame }),
      (frame, moved) => {
        setLive(null);
        if (moved) void editMarks({ op: "moveMarker", id: marker.id, at: frame });
      },
    );
  };

  const grabRegion = (event: PointerEvent, region: Region, part: "body" | "start" | "end") => {
    if (event.button !== 0 || !lane.current) return;
    event.stopPropagation();
    pick("region", region.id);
    const length = region.end - region.start;
    const grabbed = Math.round((event.clientX - lane.current.getBoundingClientRect().left) / pxPerFrame) - region.start;
    const targets = snapTargets([region.id], ["origin"]);
    const place = (frame: number) => {
      if (part === "start") return { start: Math.min(frame, region.end - 1), end: region.end };
      if (part === "end") return { start: region.start, end: Math.max(frame, region.start + 1) };
      const first = Math.max(0, snapBlockStart(frame - grabbed, length, targets, { alt: event.altKey }));
      return { start: first, end: first + length };
    };
    let last = place(region.start);
    follow(
      event,
      lane.current,
      pxPerFrame,
      // The body snaps as a block, from the pointer's raw place.
      part === "body" ? null : targets,
      (frame) => {
        last = place(frame);
        setLive({ id: region.id, at: last.start, end: last.end });
      },
      (_frame, moved) => {
        setLive(null);
        if (moved) void editMarks({ op: "setRegion", id: region.id, ...last });
      },
    );
  };

  const grabChange = (event: PointerEvent, change: TempoChange) => {
    if (event.button !== 0 || !lane.current) return;
    event.stopPropagation();
    pick("tempo", change.id);
    follow(
      event,
      lane.current,
      pxPerFrame,
      null,
      (frame) => setLive({ id: change.id, at: frame }),
      (frame, moved) => {
        setLive(null);
        if (moved) void editMarks({ op: "moveTempoChange", id: change.id, at: frame });
      },
    );
  };

  // A press on the empty lane draws a region; without a drag it only lets go of what was picked.
  const draw = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !lane.current || event.target !== event.currentTarget) return;
    selectMark(null);
    const from = Math.max(0, snapFrame(Math.round((event.clientX - lane.current.getBoundingClientRect().left) / pxPerFrame), [], { alt: event.altKey }));
    follow(
      event,
      lane.current,
      pxPerFrame,
      snapTargets([], ["origin"]),
      (frame) => setDrawing({ start: Math.min(from, frame), end: Math.max(from, frame) }),
      (frame, moved) => {
        setDrawing(null);
        if (moved && frame !== from) {
          void editMarks({ op: "addRegion", start: Math.min(from, frame), end: Math.max(from, frame), label: t("marks.region.label") });
        }
      },
    );
  };

  const markerMenu = (marker: Marker): MenuItem[] => [
    { label: t("marks.rename"), run: () => renameMark({ kind: "marker", id: marker.id }) },
    { label: t("marks.color"), submenu: colorMenu(t, marker.color, (color) => void editMarks({ op: "setMarkerColor", id: marker.id, color })) },
    "-",
    { label: t("marks.delete"), run: () => void editMarks({ op: "removeMarker", id: marker.id }) },
  ];
  const regionMenu = (region: Region): MenuItem[] => [
    { label: t("marks.rename"), run: () => renameMark({ kind: "region", id: region.id }) },
    { label: t("marks.color"), submenu: colorMenu(t, region.color, (color) => void editMarks({ op: "setRegionColor", id: region.id, color })) },
    "-",
    { label: t("marks.delete"), run: () => void editMarks({ op: "removeRegion", id: region.id }) },
  ];
  const laneMenu = (frame: number): MenuItem[] => [
    { label: t("marks.menu.addMarker"), run: () => void editMarks({ op: "addMarker", at: frame, label: t("marks.marker.label") }) },
    {
      label: t("marks.menu.addTempo"),
      run: () => {
        void addTempoChangeAt(frame);
      },
    },
  ];

  const name = (value: string, kind: Mark["kind"], id: string, op: (text: string) => MarksOp) =>
    renaming?.kind === kind && renaming.id === id ? (
      <InlineName value={value} label={t("marks.name")} onCommit={(text) => void editMarks(op(text))} onDone={() => renameMark(null)} />
    ) : (
      <span className="mark-name">{value}</span>
    );

  return (
    <div
      className="mark-lane"
      ref={lane}
      style={{ height: MARKER_RAIL_H, width }}
      role="group"
      aria-label={t("marks.lane")}
      onPointerDown={draw}
      onContextMenu={(event) => {
        if (event.target !== event.currentTarget || !lane.current) return;
        menuAtPointer(event, laneMenu(Math.max(0, Math.round((event.clientX - lane.current.getBoundingClientRect().left) / pxPerFrame))));
      }}
    >
      {regions.map((region) => {
        const moving = live?.id === region.id ? live : null;
        const [from, to] = [moving?.at ?? region.start, moving?.end ?? region.end];
        return (
          <div
            key={region.id}
            className="mark-region"
            style={{ left: x(from), width: Math.max(2, x(to) - x(from)), ...tint(region.color) }}
            data-selected={isSelected("region", region.id) || undefined}
            title={t("marks.region.title", { name: region.name, start: formatFrames(start + from, frameRate), end: formatFrames(start + to, frameRate) })}
            onPointerDown={(event) => grabRegion(event, region, "body")}
            onDoubleClick={() => renameMark({ kind: "region", id: region.id })}
            onContextMenu={(event) => menuAtPointer(event, regionMenu(region))}
          >
            <span className="mark-edge" data-edge="start" onPointerDown={(event) => grabRegion(event, region, "start")} />
            {name(region.name, "region", region.id, (text) => ({ op: "renameRegion", id: region.id, name: text }))}
            <span className="mark-edge" data-edge="end" onPointerDown={(event) => grabRegion(event, region, "end")} />
          </div>
        );
      })}
      {drawing && <div className="mark-region" data-drawing="" style={{ left: x(drawing.start), width: Math.max(1, x(drawing.end) - x(drawing.start)) }} />}
      {markers.map((marker) => {
        const at = live?.id === marker.id ? live.at : marker.at;
        return (
          <div
            key={marker.id}
            className="mark-marker"
            style={{ left: x(at), ...tint(marker.color) }}
            data-selected={isSelected("marker", marker.id) || undefined}
            title={t("marks.marker.title", { name: marker.name, time: formatFrames(start + at, frameRate) })}
            onPointerDown={(event) => grabMarker(event, marker)}
            onDoubleClick={() => renameMark({ kind: "marker", id: marker.id })}
            onContextMenu={(event) => menuAtPointer(event, markerMenu(marker))}
          >
            {name(marker.name, "marker", marker.id, (text) => ({ op: "renameMarker", id: marker.id, name: text }))}
          </div>
        );
      })}
      {tempoChanges.map((change) => {
        const at = live?.id === change.id ? live.at : change.at;
        return (
          <div
            key={change.id}
            className="mark-tempo"
            style={{ left: x(at) }}
            data-selected={isSelected("tempo", change.id) || undefined}
            title={isSelected("tempo", change.id) ? undefined : t("marks.tempo.title", { bpm: change.bpm, signature: `${change.beatsPerBar}/${change.beatUnit ?? 4}` })}
            onPointerDown={(event) => grabChange(event, change)}
          >
            <span className="mono">
              {change.bpm} · {change.beatsPerBar}/{change.beatUnit ?? 4}
            </span>
            {isSelected("tempo", change.id) && <TempoChangeEditor change={change} />}
          </div>
        );
      })}
    </div>
  );
}

/** Lines through the lanes at each marker, so what is under a marker can be told at a glance. */
export function MarkLines({ pxPerFrame, top }: { pxPerFrame: number; top: number }) {
  const markers = useDoc((doc) => activeTimeline(doc).markers);
  if (!markers?.length) return null;
  return (
    <div className="mark-lines" style={{ top }} aria-hidden="true">
      {markers.map((marker) => (
        <div key={marker.id} className="mark-line" style={{ left: frameX(marker.at, pxPerFrame), ...tint(marker.color) }} />
      ))}
    </div>
  );
}
