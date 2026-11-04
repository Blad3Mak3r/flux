// Runs the illustration: the dot-matrix readout counts frames and the playhead moves with it, so
// the time shown and the position drawn are one clock. Still when the visitor prefers reduced
// motion, and idle while off screen.

import { pixels } from "../lib/dots";

const FPS = 25;
const START = 60 * 60 * FPS; // 01:00:00:00
const LOOP_FRAMES = 16 * FPS;
const X0 = 20;
const X1 = 580;

const svg = document.querySelector<SVGSVGElement>("[data-readout]");
const head = document.querySelector<SVGGElement>("[data-playhead]");
const screen = svg?.closest<HTMLElement>(".screen");

if (svg && head && screen) {
  const on = [...svg.querySelectorAll<SVGGElement>("[data-slot]")].map((slot) => slot.querySelector(".dot-on")!);
  const pad = (n: number) => String(n).padStart(2, "0");

  const paint = (frame: number) => {
    const total = START + frame;
    const digits =
      pad(Math.floor(total / (3600 * FPS))) +
      pad(Math.floor(total / (60 * FPS)) % 60) +
      pad(Math.floor(total / FPS) % 60) +
      pad(total % FPS);
    on.forEach((path, i) => path.setAttribute("d", pixels(digits[i], true)));
    head.setAttribute("transform", `translate(${X0 + (frame / LOOP_FRAMES) * (X1 - X0)} 0)`);
  };

  if (!matchMedia("(prefers-reduced-motion: reduce)").matches) {
    let visible = true;
    let origin = performance.now();
    let last = -1;
    const tick = (now: number) => {
      if (!visible) return;
      const frame = Math.floor(((now - origin) / 1000) * FPS) % LOOP_FRAMES;
      if (frame !== last) {
        last = frame;
        paint(frame);
      }
      requestAnimationFrame(tick);
    };
    new IntersectionObserver(([entry]) => {
      const was = visible;
      visible = entry.isIntersecting && !document.hidden;
      if (visible && !was) {
        // Carry on from where it stopped.
        origin = performance.now() - (Math.max(last, 0) / FPS) * 1000;
        requestAnimationFrame(tick);
      }
    }).observe(screen);
    // Vermilion means live: the playhead only takes it while the clock runs.
    screen.dataset.live = "true";
    requestAnimationFrame(tick);
  }
}
