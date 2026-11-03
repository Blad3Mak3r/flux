import { useEffect } from "react";
import type { RefObject } from "react";
import { useT } from "../i18n";
import { useStore } from "../store";
import { frameX } from "../timeline/scale";
import { positionStore } from "../transport/playback";

/** The line at the current position. It keeps itself in view by turning pages, also after a jump (Stop, seek). */
export function Playhead({
  parked,
  start,
  pxPerFrame,
  scroller,
}: {
  parked: number;
  start: number;
  pxPerFrame: number;
  scroller: RefObject<HTMLDivElement | null>;
}) {
  const t = useT();
  const position = useStore(positionStore);
  const frame = position?.frame ?? parked;
  const x = frameX(frame - start, pxPerFrame);
  const playing = position?.state === "playing";

  useEffect(() => {
    const box = scroller.current;
    if (!box) return;
    const visibleLeft = box.scrollLeft;
    const visibleRight = visibleLeft + box.clientWidth;
    if (x > visibleRight - 40 || x < visibleLeft) box.scrollLeft = Math.max(0, x - 120);
  }, [x, scroller]);

  return (
    <div
      className="playhead"
      data-playing={playing}
      style={{ transform: `translateX(${x}px)` }}
      role="presentation"
      title={t("timeline.playhead")}
    />
  );
}
