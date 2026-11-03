import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent } from "react";
import { useT } from "../i18n";
import type { Project } from "../project/types";
import { nominalFps, realFps } from "../timecode";
import { RULER_H, patchReadout, readoutStore } from "../timeline/layout";
import { clampZoom, frameX } from "../timeline/scale";
import { useStore } from "../store";
import type { Peaks } from "./ClipWave";
import { Graticule } from "./Graticule";
import { Playhead } from "./Playhead";
import { Ruler } from "./Ruler";
import { GUESS_SECONDS, TrackHeader, TrackLane } from "./TrackRow";
import type { Selected } from "./TrackRow";
import { ZoomInIcon, ZoomOutIcon } from "./icons";

const ZOOM_STEP = 1.25;
/** Room past the last item, so there is always somewhere to drop the next one. */
const TAIL = 1.15;
const MIN_SECONDS = 120;

function contentEnd(project: Project, durations: Record<string, number>, guess: number): number {
  let end = 0;
  for (const track of project.tracks) {
    if (track.kind.type === "audio") {
      for (const clip of track.kind.clips) end = Math.max(end, clip.start + (durations[clip.asset] ?? guess));
    } else {
      for (const trigger of track.kind.triggers) end = Math.max(end, trigger.at + 25);
    }
  }
  return end;
}

/** Shades the measured span and draws the cursor under the pointer across all lanes. */
function Overlays({ pxPerFrame }: { pxPerFrame: number }) {
  const { hover, spanA, spanB } = useStore(readoutStore);
  const spanStart = spanA !== null && spanB !== null ? Math.min(spanA, spanB) : null;
  const spanEnd = spanA !== null && spanB !== null ? Math.max(spanA, spanB) : null;
  return (
    <>
      {spanStart !== null && spanEnd !== null && spanEnd > spanStart && (
        <div
          className="span-shade"
          style={{ left: frameX(spanStart, pxPerFrame), width: frameX(spanEnd, pxPerFrame) - frameX(spanStart, pxPerFrame) }}
        />
      )}
      {hover !== null && (
        <div className="hover-line" style={{ transform: `translateX(${frameX(hover, pxPerFrame)}px)` }} />
      )}
    </>
  );
}

export function Timeline({
  project,
  durations,
  waveforms,
  parked,
  selection,
  pxPerFrame,
  zoomPercent,
  onZoom,
  onSelect,
  onMove,
  onToggleMute,
  onSeek,
}: {
  project: Project;
  durations: Record<string, number>;
  waveforms: Record<string, Peaks>;
  parked: number;
  selection: Selected | null;
  pxPerFrame: number;
  zoomPercent: number;
  onZoom: (pxPerFrame: number) => void;
  onSelect: (selection: Selected | null) => void;
  onMove: (id: string, position: number) => void;
  onToggleMute: (trackId: string) => void;
  onSeek: (absoluteFrame: number) => void;
}) {
  const t = useT();
  const scroller = useRef<HTMLDivElement>(null);
  const headers = useRef<HTMLDivElement>(null);
  const [viewWidth, setViewWidth] = useState(900);
  const [scrollbarHeight, setScrollbarHeight] = useState(0);
  const { frameRate, start } = project.timecode;
  const fps = nominalFps(frameRate);
  const guessLength = GUESS_SECONDS * fps;

  useEffect(() => {
    const box = scroller.current;
    if (!box) return;
    const measure = () => {
      setViewWidth(box.clientWidth);
      setScrollbarHeight(box.offsetHeight - box.clientHeight);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    measure();
    return () => observer.disconnect();
  }, []);

  // Ctrl + wheel (and trackpad pinch) zooms around the pointer. React's wheel listener is passive,
  // so this one is native.
  const zoomRef = useRef({ pxPerFrame, onZoom });
  zoomRef.current = { pxPerFrame, onZoom };
  useEffect(() => {
    const box = scroller.current;
    if (!box) return;
    const onWheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return;
      event.preventDefault();
      const { pxPerFrame: current, onZoom: setZoom } = zoomRef.current;
      const next = clampZoom(current * (event.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP));
      const pointerX = event.clientX - box.getBoundingClientRect().left;
      const frameUnderPointer = (box.scrollLeft + pointerX) / current;
      setZoom(next);
      requestAnimationFrame(() => {
        box.scrollLeft = frameUnderPointer * next - pointerX;
      });
    };
    box.addEventListener("wheel", onWheel, { passive: false });
    return () => box.removeEventListener("wheel", onWheel);
  }, []);

  const assetNames = useMemo(
    () => Object.fromEntries(project.assets.map((asset) => [asset.id, asset.name])),
    [project.assets],
  );

  const end = contentEnd(project, durations, guessLength);
  const contentWidth = Math.ceil(
    Math.max(end * TAIL, MIN_SECONDS * fps) * pxPerFrame,
    // Never narrower than the window, so the graticule fills the screen.
  );
  const laneWidth = Math.max(contentWidth, viewWidth);

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const left = event.currentTarget.getBoundingClientRect().left;
    patchReadout({ hover: Math.max(0, Math.round((event.clientX - left) / pxPerFrame)) });
  };

  return (
    <section className="screen" aria-labelledby="tracks-legend">
      <div className="screen-bar">
        <h2 id="tracks-legend" className="legend">
          {t("tracks.legend")}
        </h2>
      </div>

      <div className="screen-body" style={{ "--scrollbar-h": `${scrollbarHeight}px` } as CSSProperties}>
        {/* Outside the horizontal scroller, so its scrollbar spans the lanes only. */}
        <div className="screen-side">
          <div className="zoom-corner" style={{ height: RULER_H }}>
            <div className="zoom">
              <button
                type="button"
                className="icon-button"
                onClick={() => onZoom(clampZoom(pxPerFrame / ZOOM_STEP))}
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
                onClick={() => onZoom(clampZoom(pxPerFrame * ZOOM_STEP))}
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
              {project.tracks.map((track) => (
                <TrackHeader key={track.id} track={track} onToggleMute={onToggleMute} />
              ))}
              <div className="filler">
                <p className="hint">{t("tracks.hint")}</p>
              </div>
            </div>
          </div>
        </div>

        <div
          className="screen-scroll"
          ref={scroller}
          onScroll={(event) => {
            if (headers.current) headers.current.scrollTop = event.currentTarget.scrollTop;
          }}
          onPointerDown={(event) => event.target === event.currentTarget && onSelect(null)}
        >
          <div
            className="screen-inner"
            style={{ width: laneWidth }}
            onPointerMove={onPointerMove}
            onPointerLeave={() => patchReadout({ hover: null })}
          >
            <Graticule width={laneWidth} pxPerFrame={pxPerFrame} rate={frameRate} />

            <div className="ruler-row" style={{ height: RULER_H }}>
              <Ruler
                width={laneWidth}
                pxPerFrame={pxPerFrame}
                rate={frameRate}
                start={start}
                onSeek={(relative) => onSeek(start + relative)}
              />
            </div>

            {project.tracks.map((track) => (
              <TrackLane
                key={track.id}
                track={track}
                assetNames={assetNames}
                durations={durations}
                waveforms={waveforms}
                guessLength={guessLength}
                fps={realFps(frameRate)}
                pxPerFrame={pxPerFrame}
                selected={selection}
                onSelect={onSelect}
                onMove={onMove}
              />
            ))}

            {project.tracks.length === 0 && (
              <div className="empty-state">
                <p className="empty-title">{t("track.empty.title")}</p>
                <p>{t("track.empty.body")}</p>
              </div>
            )}

            <Overlays pxPerFrame={pxPerFrame} />
            <Playhead parked={parked} start={start} pxPerFrame={pxPerFrame} scroller={scroller} />
          </div>
        </div>
      </div>
    </section>
  );
}
