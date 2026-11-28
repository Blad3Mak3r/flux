// A cell of the grid (a clip slot) and the head of a column.

import { useState } from "react";
import { InlineName } from "../components/InlineName";
import { Marquee } from "../components/Marquee";
import { useT } from "../i18n";
import { MediaProgressBar } from "../media/progress";
import { focusMedia } from "../media/store";
import type { VjClip, VjColumn, VjLayer } from "../project/types";
import { useDoc } from "../state/store";
import { startDrag, useDropTarget } from "../state/pointerDrag";
import { useFileDrop } from "../state/fileDrop";
import { openMenu } from "../state/ui";
import { edit, launch, launchColumn, release, selectClip, selectSlot, stopLayer, useVj } from "./store";
import { round2 } from "../format";
import { clipName } from "./names";
import { mappable } from "./mapModel";
import { EffectBadge, carriesEffect, dropEffect } from "./EffectStack";
import { useVideoState, VideoState, stripOf, Thumb } from "./VideoThumb";
import { NO_VIDEOS, DRAG } from "./shared";
import type { Dragged } from "./shared";
import { PlayTime } from "./LayerFader";

/** The clip of `layer` in the cell under `column`, the `at`-th of the deck shown. */
function clipAt(layer: VjLayer, column: VjColumn, at: number): VjClip | undefined {
  return layer.clips?.find((clip) => (layer.global ? clip.slot === at : clip.column === column.id));
}

export function Cell({ layer, column, at }: { layer: VjLayer; column: VjColumn; at: number }) {
  const t = useT();
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const thumbs = useDoc((doc) => doc.thumbs);
  const [over, setOver] = useState(false);
  const clip = clipAt(layer, column, at);
  // Only the cell whose clip plays, or is selected, draws again when that changes.
  const selected = useVj((vj) => clip !== undefined && vj.clip === clip.id);
  const live = useVj((vj) => clip !== undefined && vj.play.playing.some((p) => p.layer === layer.id && p.clip === clip.id));
  const state = useVideoState(clip?.asset);
  const slotSelected = useVj((vj) => vj.slot === `${layer.id}/${column.id}`);

  const dropping = useDropTarget({
    accepts: (data) => DRAG in data || (clip !== undefined && carriesEffect(data, { clip: clip.id })),
    over: setOver,
    drop: (data) => {
      // An effect lands on the clip of the cell, and on nothing in an empty one.
      if (clip && dropEffect(data, { clip: clip.id })) return;
      if (!(DRAG in data)) return;
      const dragged = JSON.parse(data[DRAG]) as Dragged;
      if ("clip" in dragged) {
        if (dragged.clip !== clip?.id) void edit({ op: "moveClip", clip: dragged.clip, layer: layer.id, column: column.id });
      } else if ("generator" in dragged) {
        void edit({ op: "placeGenerator", generator: { kind: dragged.generator }, layer: layer.id, column: column.id });
      } else void edit({ op: "placeVideo", asset: dragged.asset, layer: layer.id, column: column.id });
    },
  });
  // A file from the file explorer is over this cell.
  const filed = useFileDrop((drop) => drop.cell?.layer === layer.id && drop.cell.column === column.id);
  const lit = over || filed;

  if (!clip) {
    // As on a media server, an empty cell stops its layer, over the layer's transition.
    return (
      <div className={`vj-slot${lit ? " over" : ""}${slotSelected ? " sel" : ""}`} data-drop-layer={layer.id} data-drop-column={column.id} ref={dropping}>
        <div className="vj-slot-pic" title={t("vj.cell.empty")} onPointerDown={(event) => { if (event.button === 0) void stopLayer(layer.id); }} />
        <div className="vj-pick" title={t("vj.cell.select")} onPointerDown={(event) => { if (event.button === 0) selectSlot(layer.id, column.id); }} />
      </div>
    );
  }
  const name = clipName(t, clip, videos);
  const piano = clip.trigger === "piano";
  const motion = transportBadge(clip);
  return (
    <div
      className={`vj-clip${live ? " on-air" : ""}${selected ? " sel" : ""}${lit ? " over" : ""}${state === "offline" ? " offline" : ""}`}
      {...mappable({ kind: "clipLaunch", clip: clip.id })}
      data-drop-layer={layer.id} data-drop-column={column.id}
      ref={dropping}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        startDrag(event, { [DRAG]: JSON.stringify({ clip: clip.id } satisfies Dragged) }, name);
        // An offline clip has nothing to play: its click opens the media page on it (a drag swallows the click).
        if (state !== "offline") void launch(clip.id, layer.id);
      }}
      onClick={() => { if (state === "offline" && clip.asset) focusMedia(clip.asset); }}
      onPointerUp={() => { if (piano) void release(clip.id); }}
      onPointerLeave={(event) => { if (piano && event.buttons === 1) void release(clip.id); }}
      onContextMenu={(event) => {
        event.preventDefault();
        selectClip(clip.id, layer.id);
        openMenu({ x: event.clientX, y: event.clientY, items: [
          { label: t("vj.clip.remove"), run: () => void edit({ op: "removeClip", clip: clip.id }) },
        ] });
      }}
    >
      <div className="vj-pic">
        <Thumb strip={stripOf(thumbs, clip)} generator={clip.generator} />
        <span className="vj-shade" />
        <EffectBadge count={clip.effects?.length ?? 0} />
        {live && <PlayTime layer={layer.id} clip={clip.id} countdown="cell" />}
        {state && <VideoState state={state} asset={clip.asset} />}
        {clip.asset && <MediaProgressBar asset={clip.asset} />}
        {/* A folded row has no room to click the cell: its square launches the clip, as the cell does (the pointer reaches the cell). */}
        {layer.collapsed && (
          <button type="button" className={`vj-go${live ? " on" : ""}`} aria-pressed={live}
            title={t("vj.clip.launchNamed", { clip: name })} aria-label={t("vj.clip.launchNamed", { clip: name })}
            onKeyDown={(event) => { if ((event.key === "Enter" || event.key === " ") && !event.repeat) { event.preventDefault(); void launch(clip.id, layer.id); } }}
            onKeyUp={(event) => { if (piano && (event.key === "Enter" || event.key === " ")) void release(clip.id); }} />
        )}
      </div>
      {/* The name strip selects the clip without launching it. */}
      <div className="vj-pick" aria-label={t("vj.clip.select")}
        onPointerDown={(event) => { event.stopPropagation(); if (event.button === 0) selectClip(clip.id, layer.id); }}
        onPointerUp={(event) => event.stopPropagation()}>
        <Marquee className="vj-cn">{name}</Marquee>
        <span className="vj-cm">{t(`vj.trigger.short.${clip.trigger ?? "normal"}`)}{clip.playback && clip.playback !== "loop" ? ` · ${t(`vj.playback.short.${clip.playback}`)}` : ""}{motion ? ` · ${motion}` : ""}</span>
      </div>
    </div>
  );
}

/** What shows on a cell of a clip that does not play forwards at its own speed: ◀ or ❚❚, then × the speed or the beats of a pass. */
function transportBadge(clip: VjClip): string {
  const parts: string[] = [];
  if (clip.direction === "reverse") parts.push("◀");
  if (clip.direction === "pause") parts.push("❚❚");
  if (clip.tempo?.kind === "beats") parts.push(`${clip.tempo.beats}♩`);
  else if (clip.speed !== undefined && clip.speed !== 1) parts.push(`×${round2(clip.speed)}`);
  return parts.join(" ");
}

export function ColumnHead({ column, at, live }: { column: VjColumn; at: number; live: boolean }) {
  const t = useT();
  const [renaming, setRenaming] = useState(false);
  return (
    <button
      type="button"
      className={`vj-col${live ? " on-air" : ""}`}
      {...mappable({ kind: "columnLaunch", column: column.id })}
      title={t("vj.column.launch")}
      onClick={() => void launchColumn(column.id)}
      onDoubleClick={() => setRenaming(true)}
      onContextMenu={(event) => {
        event.preventDefault();
        openMenu({ x: event.clientX, y: event.clientY, items: [
          { label: t("vj.column.rename"), run: () => setRenaming(true) },
          { label: t("vj.column.remove"), run: () => void edit({ op: "removeColumn", column: column.id }) },
        ] });
      }}
    >
      <span className="vj-num">{at + 1}</span>
      {renaming ? (
        <InlineName value={column.name ?? ""} label={t("vj.column.rename")} onDone={() => setRenaming(false)}
          onCommit={(text) => void edit({ op: "renameColumn", column: column.id, name: text })} />
      ) : (
        <Marquee>{column.name ?? ""}</Marquee>
      )}
    </button>
  );
}
