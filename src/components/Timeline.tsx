import { addOperator, useVoice } from "../voice/store";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent } from "react";
import { useT } from "../i18n";
import { ViewSwitch } from "../live/ViewSwitch";
import { TimelineStrip } from "./TimelineStrip";
import { FollowToggle } from "./FollowToggle";
import { SnapControl } from "./SnapControl";
import { TempoBar } from "./TempoBar";
import type { Timeline as TimelineData } from "../project/types";
import { menuAtPointer, select, setZoom, useUi, zoomBy } from "../state/ui";
import { activeTimeline } from "../state/doc";
import type { AssetAudio } from "../state/doc";
import { useDoc, useTimeline } from "../state/store";
import { nominalFps, realFps } from "../timecode";
import { GUESS_SECONDS, clipLength } from "../timeline/clip";
import { RULER_H, patchReadout, setViewport, useReadout } from "../timeline/layout";
import { dropIndex, dropLineY } from "../timeline/reorder";
import { DEFAULT_PX_PER_FRAME, frameX } from "../timeline/scale";
import type { BeatGrid } from "../timeline/scale";
import { reorderTrack } from "../state/actions";
import { snapFrame } from "../state/snap";
import { addCueTrack } from "../state/cues";
import { addNoteAt, importAudioFiles } from "../state/workflows";
import { seek } from "../transport/playback";
import { Graticule } from "./Graticule";
import { Playhead } from "./Playhead";
import { Ruler } from "./Ruler";
import { TrackHeader, TrackLane, laneHeight } from "./TrackRow";
import { PlusIcon, ZoomInIcon, ZoomOutIcon } from "./icons";
import { emptyMenu } from "./menus";

const ZOOM_STEP = 1.25;
/** Room past the last item, so there is always somewhere to drop the next one. */
const TAIL = 1.15;
const MIN_SECONDS = 120;

function contentEnd(timeline: TimelineData, audio: Record<string, AssetAudio>, fps: number, guess: number): number {
  let end = 0;
  for (const track of timeline.tracks) {
    if (track.kind.type === "audio") {
      for (const clip of track.kind.clips) end = Math.max(end, clip.start + clipLength(clip, audio[clip.asset], fps, guess));
    } else if (track.kind.type === "notes") {
      for (const note of track.kind.notes) end = Math.max(end, note.at + (note.length ?? 25));
    } else if (track.kind.type === "operator") {
      for (const order of track.kind.orders) {
        const view = useVoice.getState().views[order.id];
        end = Math.max(end, view?.start !== null && view?.start !== undefined ? view.start + (view.length ?? 0) : order.at);
      }
    } else {
      for (const event of track.kind.events) end = Math.max(end, event.at + 25);
    }
  }
  return end;
}

/** Shades the measured span. */
function SpanShade({ pxPerFrame }: { pxPerFrame: number }) {
  const spanA = useReadout((readout) => readout.spanA);
  const spanB = useReadout((readout) => readout.spanB);
  if (spanA === null || spanB === null || spanA === spanB) return null;
  const from = frameX(Math.min(spanA, spanB), pxPerFrame);
  return <div className="span-shade" style={{ left: from, width: frameX(Math.max(spanA, spanB), pxPerFrame) - from }} />;
}

/**
 * The cursor under the pointer, across all lanes. It follows the pointer by writing its own style
 * from the store the moment the readout changes: going through a render would put it a frame or two
 * behind the pointer, which is what makes it feel slow.
 */
function HoverLine({ pxPerFrame }: { pxPerFrame: number }) {
  const line = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = line.current;
    if (!element) return;
    const place = (hover: number | null) => {
      element.hidden = hover === null;
      if (hover !== null) element.style.transform = `translateX(${frameX(hover, pxPerFrame)}px)`;
    };
    place(useReadout.getState().hover);
    return useReadout.subscribe((readout, before) => {
      if (readout.hover !== before.hover) place(readout.hover);
    });
  }, [pxPerFrame]);
  return <div ref={line} className="hover-line" hidden />;
}

/**
 * Where the magnet is holding the item or the playhead being dragged, across all lanes. Solid on a
 * line of the grid, dashed on something else, so the kind does not ride on a colour.
 */
function SnapGuideLine({ pxPerFrame }: { pxPerFrame: number }) {
  const guide = useUi((ui) => ui.snapGuide);
  if (!guide) return null;
  return (
    <div
      className="snap-guide"
      data-kind={guide.kind}
      style={{ transform: `translateX(${frameX(guide.frame, pxPerFrame)}px)` }}
      aria-hidden="true"
    />
  );
}

export function Timeline() {
  const t = useT();
  const scroller = useRef<HTMLDivElement>(null);
  const headers = useRef<HTMLDivElement>(null);
  const [viewWidth, setViewWidth] = useState(900);
  const [scrollbarHeight, setScrollbarHeight] = useState(0);
  const assets = useDoc((doc) => doc.project.assets);
  const timeline = useTimeline();
  const audio = useDoc((doc) => doc.audio);
  useVoice((v) => v.views);
  const pxPerFrame = useUi((ui) => ui.zoom);
  const bars = useUi((ui) => ui.ruler === "bars");
  const { frameRate, start } = timeline.timecode;
  const { bpm, beatsPerBar } = timeline.tempo;
  const exactFps = realFps(frameRate);
  // Kept by identity while nothing it is made of changes, so the ticks are not redrawn for nothing.
  const grid = useMemo<BeatGrid | null>(
    () => (bars ? { bpm, beatsPerBar, fps: exactFps } : null),
    [bars, bpm, beatsPerBar, exactFps],
  );
  const fps = nominalFps(frameRate);
  const guessLength = GUESS_SECONDS * fps;
  const zoomPercent = Math.round((pxPerFrame / DEFAULT_PX_PER_FRAME) * 100);
  // The track being dragged by its header, and the index it would take if dropped now. The header
  // itself follows the pointer through its own style (`dy` changes every move; this state only on a new slot).
  const [drag, setDrag] = useState<{ id: string; to: number } | null>(null);

  useEffect(() => {
    const box = scroller.current;
    if (!box) return;
    const measure = () => {
      setViewWidth(box.clientWidth);
      setViewport({ left: box.scrollLeft, width: box.clientWidth });
      setScrollbarHeight(box.offsetHeight - box.clientHeight);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    measure();
    return () => observer.disconnect();
  }, []);

  // Ctrl + wheel (and trackpad pinch) zooms around the pointer. React's wheel listener is passive,
  // so this one is native.
  useEffect(() => {
    const box = scroller.current;
    if (!box) return;
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return;
      event.preventDefault();
      const current = useUi.getState().zoom;
      setZoom(current * (event.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP));
      const next = useUi.getState().zoom;
      const pointerX = event.clientX - box.getBoundingClientRect().left;
      const frameUnderPointer = (box.scrollLeft + pointerX) / current;
      requestAnimationFrame(() => {
        box.scrollLeft = frameUnderPointer * next - pointerX;
      });
    };
    box.addEventListener("wheel", onWheel, { passive: false });
    return () => box.removeEventListener("wheel", onWheel);
  }, []);

  const assetNames = useMemo(
    () => Object.fromEntries(assets.map((asset) => [asset.id, asset.name])),
    [assets],
  );

  const end = contentEnd(timeline, audio, realFps(frameRate), guessLength);
  const contentWidth = Math.ceil(
    Math.max(end * TAIL, MIN_SECONDS * fps) * pxPerFrame,
    // Never narrower than the window, so the graticule fills the screen.
  );
  const laneWidth = Math.max(contentWidth, viewWidth);

  const grabTrack = useCallback((event: PointerEvent<HTMLElement>, id: string) => {
    if (event.button !== 0 || (event.target as Element).closest("button")) return;
    const header = event.currentTarget.closest<HTMLElement>(".track-header");
    const { tracks } = activeTimeline(useDoc.getState());
    const from = tracks.findIndex((track) => track.id === id);
    if (!header || from === -1) return;
    const heights = tracks.map(laneHeight);
    const above = heights.slice(0, from).reduce((sum, height) => sum + height, 0);
    const below = heights.slice(from + 1).reduce((sum, height) => sum + height, 0);
    const startY = event.clientY;
    let to = from;
    setDrag({ id, to });

    const onMove = (move: globalThis.PointerEvent) => {
      const dy = Math.min(Math.max(move.clientY - startY, -above), below);
      header.style.transform = `translateY(${dy}px)`;
      const next = dropIndex(heights, from, dy);
      if (next !== to) {
        to = next;
        setDrag({ id, to });
      }
    };
    const onKey = (key: KeyboardEvent) => key.key === "Escape" && finish(false);
    const finish = (drop: boolean) => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("keydown", onKey);
      header.style.transform = "";
      setDrag(null);
      if (drop) reorderTrack(id, to);
    };
    const onUp = () => finish(true);
    const onCancel = () => finish(false);
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    window.addEventListener("keydown", onKey);
  }, []);

  const dragFrom = drag ? timeline.tracks.findIndex((track) => track.id === drag.id) : -1;
  const dropY = drag && drag.to !== dragFrom ? dropLineY(timeline.tracks.map(laneHeight), dragFrom, drag.to) : null;

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const left = event.currentTarget.getBoundingClientRect().left;
    patchReadout({ hover: Math.max(0, snapFrame(Math.round((event.clientX - left) / pxPerFrame))) });
  };

  return (
    <section className="screen" aria-labelledby="tracks-legend">
      <div className="screen-bar">
        <h2 id="tracks-legend" className="legend">
          {t("tracks.legend")}
        </h2>
        <TimelineStrip />
        <TempoBar />
        <SnapControl />
        <FollowToggle />
        <ViewSwitch />
      </div>

      <div className="screen-body" style={{ "--scrollbar-h": `${scrollbarHeight}px` } as CSSProperties}>
        {/* Outside the horizontal scroller, so its scrollbar spans the lanes only. */}
        <div className="screen-side">
          <div className="zoom-corner" style={{ height: RULER_H }}>
            <div className="zoom">
              <button
                type="button"
                className="icon-button"
                onClick={() => zoomBy(1 / ZOOM_STEP)}
                aria-label={t("timeline.zoomOut")}
                title={t("timeline.zoomOut")}
              >
                <ZoomOutIcon />
              </button>
              <output className="zoom-value" aria-label={t("status.zoom")}>
                {zoomPercent}%
              </output>
              <button
                type="button"
                className="icon-button"
                onClick={() => zoomBy(ZOOM_STEP)}
                aria-label={t("timeline.zoomIn")}
                title={t("timeline.zoomIn")}
              >
                <ZoomInIcon />
              </button>
            </div>
          </div>

          {/* Follows the lanes' vertical scroll; the wheel over it scrolls the lanes. */}
          <div
            className="screen-headers"
            ref={headers}
            onWheel={(event) => {
              if (!event.ctrlKey && !event.metaKey && scroller.current) scroller.current.scrollTop += event.deltaY;
            }}
          >
            <div className="headers-inner">
              {timeline.tracks.map((track) => (
                <TrackHeader key={track.id} track={track} dragging={drag?.id === track.id} onGrab={grabTrack} />
              ))}
              {dropY !== null && <div className="drop-line" style={{ top: dropY }} />}
              <div className="filler">
                <p className="hint">{t("tracks.hint")}</p>
              </div>
            </div>
          </div>

          <div className="side-add">
            <button type="button" className="add-song" onClick={() => void importAudioFiles()} title={`${t("tracks.addSong")} (I)`}>
              <PlusIcon />
              {t("tracks.addSong")}
            </button>
            <button type="button" className="add-note" onClick={() => addNoteAt(t)} title={`${t("tracks.addNote")} (N)`}>
              <PlusIcon />
              {t("tracks.addNote")}
            </button>
            <button type="button" className="add-note" onClick={() => void addOperator(t("track.kind.operator"))}><PlusIcon />{t("tracks.addOperator")}</button>
            <button type="button" className="add-note" onClick={() => void addCueTrack("midi", t("track.kind.midi"))}>
              <PlusIcon />
              {t("tracks.addMidi")}
            </button>
            <button type="button" className="add-note" onClick={() => void addCueTrack("osc", t("track.kind.osc"))}>
              <PlusIcon />
              {t("tracks.addOsc")}
            </button>
          </div>
        </div>

        <div
          className="screen-scroll"
          ref={scroller}
          onScroll={(event) => {
            setViewport({ left: event.currentTarget.scrollLeft });
            if (headers.current) headers.current.scrollTop = event.currentTarget.scrollTop;
          }}
          onPointerDown={(event) => event.target === event.currentTarget && select(null)}
          onContextMenu={(event) => menuAtPointer(event, emptyMenu(t))}
        >
          <div
            className="screen-inner"
            style={{ width: laneWidth }}
            onPointerMove={onPointerMove}
            onPointerLeave={() => patchReadout({ hover: null })}
          >
            <Graticule width={laneWidth} pxPerFrame={pxPerFrame} rate={frameRate} grid={grid} />

            <div className="ruler-row" style={{ height: RULER_H }}>
              <Ruler
                width={laneWidth}
                pxPerFrame={pxPerFrame}
                rate={frameRate}
                start={start}
                grid={grid}
                onSeek={(relative) => seek(start + relative)}
              />
            </div>

            {timeline.tracks.map((track) => (
              <TrackLane
                key={track.id}
                track={track}
                assetNames={assetNames}
                guessLength={guessLength}
                fps={realFps(frameRate)}
                pxPerFrame={pxPerFrame}
                dragging={drag?.id === track.id}
              />
            ))}
            {dropY !== null && <div className="drop-line" style={{ top: RULER_H + dropY }} />}

            {timeline.tracks.length === 0 && (
              <div className="empty-state">
                <p className="empty-title">{t("track.empty.title")}</p>
                <p>{t("track.empty.body")}</p>
              </div>
            )}

            <SpanShade pxPerFrame={pxPerFrame} />
            <HoverLine pxPerFrame={pxPerFrame} />
            <SnapGuideLine pxPerFrame={pxPerFrame} />
            <Playhead start={start} pxPerFrame={pxPerFrame} scroller={scroller} />
          </div>
        </div>
      </div>
    </section>
  );
}
