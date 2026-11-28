// The pointer gestures of the items on a lane: dragging an item, and the handles that trim a clip or set its fades.

import { operatorEdit } from "../voice/store";
import { useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent, PointerEvent } from "react";
import type { ClipOp } from "../project/api";
import type { Track } from "../project/types";
import { moveCueEvent } from "../state/cues";
import { editClip, editVideoClip } from "../state/clips";
import { snapBlockStart, snapFrame, snapTargets } from "../state/snap";
import { isSelected, select, setSnapGuide, useUi } from "../state/ui";
import { moveItems } from "../state/items";
import { groupStart, pickedIds, selectRange, setGroupDelta, toggleItem, useGroupDrag } from "../state/selection";
import type { Selected } from "../state/ui";
import type { SnapTarget } from "../timeline/snap";

/**
 * Drag an item sideways in whole frames; commit once on release so undo takes one step. `length` is
 * how many frames it lasts, so that its end can land on something as well as its start. When the item
 * is one of several selected, the whole group moves with it and the backend moves them in one step.
 */
export function useDrag(id: string, kind: Selected["kind"], position: number, length: number, pxPerFrame: number, track: Track, onClick?: () => void) {
  // A clip or a note is moved by delta, like a group; an order, an event or a video by where it lands.
  const move =
    kind === "operator"
      ? (id: string, at: number) => operatorEdit({ op: "moveOrder", id, at })
      : kind === "event"
        ? moveCueEvent
        : kind === "video"
          ? (id: string, at: number) => editVideoClip(id, "move", at)
          : null;
  const [delta, setDelta] = useState(0);
  // The group's drag is shared, so every selected item follows the one held.
  const grouped = useUi((ui) => ui.picked.has(id));
  const groupDelta = useGroupDrag((drag) => (grouped ? drag.delta : 0));
  const origin = useRef<{ x: number; position: number; targets: SnapTarget[]; floor: number; group: boolean; click: "collapse" | "toggle" | null } | null>(null);

  // Moves the item by `by` frames; with a group or ripple on, the backend moves them.
  const commit = (by: number) => {
    const ui = useUi.getState();
    if (ui.picked.has(id) || (ui.ripple !== "off" && kind !== "operator") || !move) void moveItems(ui.picked.has(id) ? pickedIds() : [id], by);
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
export function useHandle(id: string, op: ClipOp, frame: number, pxPerFrame: number, video = false) {
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
export function ClipHandle({
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
export const fadeHandleLeft = (end: number, width: number) =>
  Math.min(Math.max(end - FADE_HANDLE_PX / 2, 0), Math.max(0, width - 2 - FADE_HANDLE_PX));
