import { useEffect, useRef } from "react";
import type { RefObject } from "react";
import { useT } from "../i18n";
import { setDetached, useUi } from "../state/ui";
import { createFollower } from "../timeline/follow";
import { frameX } from "../timeline/scale";
import { displayClock } from "../transport/smooth";
import { selectFrame, selectMode, useTransport } from "../transport/store";

/** Scrolling this soon after a zoom is the zoom's own. */
const ZOOM_SETTLE_MS = 250;
/** A scroll this far from the one we set was the user's. */
const USER_SCROLL_PX = 1.5;

/**
 * The line at the current position, and the scrolling that keeps it in view.
 *
 * While the show plays it is drawn every screen frame from the display clock (the backend's
 * reports blended between messages), and the timeline flows under it once it reaches the anchor, so
 * nothing steps or pages. It writes its own style instead of rendering: a render per frame would
 * cost more than it moves. Scrolling by hand takes the view away until Follow is pressed again.
 */
export function Playhead({
  start,
  pxPerFrame,
  scroller,
}: {
  start: number;
  pxPerFrame: number;
  scroller: RefObject<HTMLDivElement | null>;
}) {
  const t = useT();
  const line = useRef<HTMLDivElement>(null);
  const playing = useTransport(selectMode) === "playing";

  useEffect(() => {
    const element = line.current;
    const box = scroller.current;
    if (!element || !box) return;

    // Zooming moves the scroll itself a frame after the zoom changes; that is not the user's hand.
    const startedAt = performance.now();
    const follower = createFollower(box.scrollLeft);
    let written = box.scrollLeft;
    let frameRequest = 0;
    let wasPlaying = false;

    const drawnFrame = (now: number): number => {
      const state = useTransport.getState();
      return (state.position?.state === "playing" ? displayClock.read(now) : null) ?? selectFrame(state);
    };

    const tick = (now: number) => {
      frameRequest = 0;
      const x = frameX(drawnFrame(now) - start, pxPerFrame);
      element.style.transform = `translateX(${x}px)`;

      const { follow, detached } = useUi.getState();
      const running = selectMode(useTransport.getState()) === "playing";
      if (follow && !detached) {
        const scroll = follower.step(x, now, {
          width: box.clientWidth,
          max: box.scrollWidth - box.clientWidth,
        });
        if (Math.abs(scroll - box.scrollLeft) >= 0.01) {
          written = scroll;
          box.scrollLeft = scroll;
          // The view rounds to a device pixel; what it kept is what it has.
          written = box.scrollLeft;
        }
      }
      // Playing keeps drawing; so does a slide that has not settled.
      if (running || (follow && !detached && follower.sliding)) request();
    };

    const request = () => {
      if (!frameRequest) frameRequest = requestAnimationFrame(tick);
    };

    const onScroll = () => {
      if (Math.abs(box.scrollLeft - written) <= USER_SCROLL_PX) return;
      const ui = useUi.getState();
      if (performance.now() - startedAt < ZOOM_SETTLE_MS) {
        written = box.scrollLeft;
        follower.sync(box.scrollLeft);
        return;
      }
      // Playing is when a hand on the scroll means "let me look elsewhere"; stopped, it is just looking.
      if (ui.follow && selectMode(useTransport.getState()) === "playing") setDetached(true);
      written = box.scrollLeft;
      follower.sync(box.scrollLeft);
    };
    box.addEventListener("scroll", onScroll, { passive: true });

    const unsubscribeTransport = useTransport.subscribe((state, before) => {
      const mode = selectMode(state);
      if (mode === "playing" && !wasPlaying) setDetached(false);
      wasPlaying = mode === "playing";
      if (selectFrame(state) !== selectFrame(before) || mode !== selectMode(before)) request();
    });
    const unsubscribeUi = useUi.subscribe((ui, before) => {
      if (ui.follow !== before.follow || ui.detached !== before.detached) {
        follower.sync(box.scrollLeft);
        request();
      }
    });
    const observer = new ResizeObserver(request);
    observer.observe(box);

    wasPlaying = selectMode(useTransport.getState()) === "playing";
    follower.sync(box.scrollLeft);
    request();
    return () => {
      cancelAnimationFrame(frameRequest);
      box.removeEventListener("scroll", onScroll);
      unsubscribeTransport();
      unsubscribeUi();
      observer.disconnect();
    };
  }, [start, pxPerFrame, scroller]);

  return <div ref={line} className="playhead" data-playing={playing} role="presentation" title={t("timeline.playhead")} />;
}
