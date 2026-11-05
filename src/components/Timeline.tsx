import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent } from "react";
import { useT } from "../i18n";
import { ViewSwitch } from "../live/ViewSwitch";
import { FollowToggle } from "./FollowToggle";
import type { Project } from "../project/types";
import { select, setZoom, useUi, zoomBy } from "../state/ui";
import type { AssetAudio } from "../state/doc";
import { useDoc } from "../state/store";
import { nominalFps, realFps } from "../timecode";
import { RULER_H, patchReadout, useReadout } from "../timeline/layout";
import { DEFAULT_PX_PER_FRAME, frameX } from "../timeline/scale";
import { seek } from "../transport/playback";
import { Graticule } from "./Graticule";
import { Playhead } from "./Playhead";
import { Ruler } from "./Ruler";
import { GUESS_SECONDS, TrackHeader, TrackLane, clipLength } from "./TrackRow";
import { ZoomInIcon, ZoomOutIcon } from "./icons";

const ZOOM_STEP = 1.25;
/** Room past the last item, so there is always somewhere to drop the next one. */
const TAIL = 1.15;
const MIN_SECONDS = 120;

function contentEnd(project: Project, audio: Record<string, AssetAudio>, fps: number, guess: number): number {
  let end = 0;
  for (const track of project.tracks) {
    if (track.kind.type === "audio") {
      for (const clip of track.kind.clips) end = Math.max(end, clip.start + clipLength(audio[clip.asset], fps, guess));
    } else {
      for (const trigger of track.kind.triggers) end = Math.max(end, trigger.at + 25);
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

export function Timeline() {
  const t = useT();
  const scroller = useRef<HTMLDivElement>(null);
  const headers = useRef<HTMLDivElement>(null);
  const [viewWidth, setViewWidth] = useState(900);
  const [scrollbarHeight, setScrollbarHeight] = useState(0);
  const project = useDoc((doc) => doc.project);
  const audio = useDoc((doc) => doc.audio);
  const pxPerFrame = useUi((ui) => ui.zoom);
  const { frameRate, start } = project.timecode;
  const fps = nominalFps(frameRate);
  const guessLength = GUESS_SECONDS * fps;
  const zoomPercent = Math.round((pxPerFrame / DEFAULT_PX_PER_FRAME) * 100);

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
    () => Object.fromEntries(project.assets.map((asset) => [asset.id, asset.name])),
    [project.assets],
  );

  const end = contentEnd(project, audio, realFps(frameRate), guessLength);
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
              {project.tracks.map((track) => (
                <TrackHeader key={track.id} track={track} />
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
          onPointerDown={(event) => event.target === event.currentTarget && select(null)}
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
                onSeek={(relative) => seek(start + relative)}
              />
            </div>

            {project.tracks.map((track) => (
              <TrackLane
                key={track.id}
                track={track}
                assetNames={assetNames}
                guessLength={guessLength}
                fps={realFps(frameRate)}
                pxPerFrame={pxPerFrame}
              />
            ))}

            {project.tracks.length === 0 && (
              <div className="empty-state">
                <p className="empty-title">{t("track.empty.title")}</p>
                <p>{t("track.empty.body")}</p>
              </div>
            )}

            <SpanShade pxPerFrame={pxPerFrame} />
            <HoverLine pxPerFrame={pxPerFrame} />
            <Playhead start={start} pxPerFrame={pxPerFrame} scroller={scroller} />
          </div>
        </div>
      </div>
    </section>
  );
}
