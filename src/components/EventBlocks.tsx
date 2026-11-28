// The other things a lane holds: notes, MIDI and OSC events, DMX scenes and the orders of an operator.

import { useVoice } from "../voice/store";
import type { VoiceOrder } from "../project/types";
import { useT } from "../i18n";
import type { DmxScene, MidiEvent, Note, OscEvent, Track } from "../project/types";
import { useDoc } from "../state/store";
import { isSelected, menuAtPointer, select, useUi } from "../state/ui";
import { frameX } from "../timeline/scale";
import { ClipWave } from "./ClipWave";
import { eventMenu, noteMenu, operatorOrderMenu } from "./menus";
import { useDrag } from "./laneGestures";

/**
 * A note on its lane: a line at its start with the text beside it. One with a length is drawn as a
 * box as long as it lasts, so what is on show at a point of the timeline can be read off the ruler.
 */
export function NoteBlock({ track, note, pxPerFrame }: { track: Track; note: Note; pxPerFrame: number }) {
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
export function EventBlock({ track, event, pxPerFrame }: { track: Track; event: MidiEvent | OscEvent; pxPerFrame: number }) {
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
export function SceneBlock({ track, scene, pxPerFrame }: { track: Track; scene: DmxScene; pxPerFrame: number }) {
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

export function OrderBlock({ track, order, fps, pxPerFrame }: { track: Track; order: VoiceOrder; fps: number; pxPerFrame: number }) {
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
