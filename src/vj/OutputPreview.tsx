// The picture of the Advanced Output: the live frame, and the drawing of a screen with its slices and masks over it.

import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef } from "react";
import type { Mask, Point, Slice } from "../video/mapping";
import { bindOutput } from "./store";
import { picture } from "./picture";

/** Each slice is drawn in this many strips each way, so its perspective bends as the output's does. */
const STEPS = 8;

/**
 * The latest picture of the composition, kept in a canvas for the stages to draw from, and a way to
 * hear when it changes without the whole window drawing again.
 */
export function useLivePicture(screenId: string) {
  const source = useMemo(() => document.createElement("canvas"), []);
  // The test cards the backend lays over the picture, small and clear where there is none.
  const card = useMemo(() => document.createElement("canvas"), []);
  const listeners = useRef(new Set<() => void>());
  const paint = () => {
    const context = source.getContext("2d");
    if (context && card.width > 0 && source.width > 0) context.drawImage(card, 0, 0, source.width, source.height);
  };
  useEffect(
    () =>
      bindOutput((picture) => {
        const context = source.getContext("2d");
        if (!context) return;
        if (picture) {
          if (source.width !== picture.width || source.height !== picture.height) {
            source.width = picture.width;
            source.height = picture.height;
          }
          context.putImageData(picture, 0, 0);
        } else context.clearRect(0, 0, source.width, source.height);
        paint();
        listeners.current.forEach((draw) => draw());
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [source, card],
  );
  useEffect(() => {
    let stopped = false;
    const ask = () =>
      void invoke<ArrayBuffer>("video_card_preview", { screen: screenId }).then(
        (bytes) => {
          if (stopped) return;
          const image = picture(bytes);
          if (!image) card.width = 0;
          else {
            card.width = image.width;
            card.height = image.height;
            card.getContext("2d")?.putImageData(image, 0, 0);
            if (source.width === 0) {
              source.width = image.width;
              source.height = image.height;
            }
          }
          paint();
          listeners.current.forEach((draw) => draw());
        },
        () => undefined,
      );
    ask();
    const timer = window.setInterval(ask, 700);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [screenId, card, source]);
  const subscribe = (draw: () => void) => {
    listeners.current.add(draw);
    return () => void listeners.current.delete(draw);
  };
  return { source, subscribe };
}

export type Live = ReturnType<typeof useLivePicture>;

/** Where on `screen` the point (`u`, `v`) of a slice lands, 0 to 1 across it, in perspective. */
function project(corners: Slice["corners"], u: number, v: number): Point {
  const [p0, p1, p2, p3] = corners;
  const dx1 = p1.x - p2.x, dx2 = p3.x - p2.x, dx3 = p0.x - p1.x + p2.x - p3.x;
  const dy1 = p1.y - p2.y, dy2 = p3.y - p2.y, dy3 = p0.y - p1.y + p2.y - p3.y;
  const det = dx1 * dy2 - dx2 * dy1;
  const g = det === 0 ? 0 : (dx3 * dy2 - dx2 * dy3) / det;
  const h = det === 0 ? 0 : (dx1 * dy3 - dx3 * dy1) / det;
  const w = g * u + h * v + 1;
  const x = (p1.x - p0.x + g * p1.x) * u + (p3.x - p0.x + h * p3.x) * v + p0.x;
  const y = (p1.y - p0.y + g * p1.y) * u + (p3.y - p0.y + h * p3.y) * v + p0.y;
  return { x: x / w, y: y / w };
}

/** Draws the triangle `s` of `image` onto the triangle `d` of `context`. */
function triangle(context: CanvasRenderingContext2D, image: CanvasImageSource, s: Point[], d: Point[]) {
  const [s0, s1, s2] = s;
  const [d0, d1, d2] = d;
  const u1x = s1.x - s0.x, u1y = s1.y - s0.y, u2x = s2.x - s0.x, u2y = s2.y - s0.y;
  const det = u1x * u2y - u2x * u1y;
  if (det === 0) return;
  const v1x = d1.x - d0.x, v1y = d1.y - d0.y, v2x = d2.x - d0.x, v2y = d2.y - d0.y;
  const a = (v1x * u2y - v2x * u1y) / det;
  const c = (v2x * u1x - v1x * u2x) / det;
  const b = (v1y * u2y - v2y * u1y) / det;
  const e = (v2y * u1x - v1y * u2x) / det;
  context.save();
  context.beginPath();
  // A hair larger than the triangle, so the seams between strips do not show.
  const mid = { x: (d0.x + d1.x + d2.x) / 3, y: (d0.y + d1.y + d2.y) / 3 };
  [d0, d1, d2].forEach((p, i) => {
    const len = Math.hypot(p.x - mid.x, p.y - mid.y) || 1;
    const q = { x: p.x + ((p.x - mid.x) / len) * 1, y: p.y + ((p.y - mid.y) / len) * 1 };
    if (i === 0) context.moveTo(q.x, q.y);
    else context.lineTo(q.x, q.y);
  });
  context.closePath();
  context.clip();
  context.setTransform(a, b, c, e, d0.x - a * s0.x - c * s0.y, d0.y - b * s0.x - e * s0.y);
  context.drawImage(image, 0, 0);
  context.restore();
}

/**
 * What one screen shows, drawn from the live composition: each slice cut out and placed on its four
 * corners, the masks over them in black. Soft edges are left to the real output.
 */
export function drawScreen(context: CanvasRenderingContext2D, source: HTMLCanvasElement, slices: Slice[], masks: Mask[]) {
  const { width, height } = context.canvas;
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.fillStyle = "#000";
  context.fillRect(0, 0, width, height);
  if (source.width === 0 || source.height === 0) return;
  if (slices.length === 0) {
    // No slices: the whole picture, fitted to the screen.
    const scale = Math.min(width / source.width, height / source.height);
    const w = source.width * scale, h = source.height * scale;
    context.drawImage(source, (width - w) / 2, (height - h) / 2, w, h);
  }
  for (const slice of slices) {
    if (slice.hidden) continue;
    const r = slice.input;
    const from = (u: number, v: number) => ({ x: (r.x + r.width * u) * source.width, y: (r.y + r.height * v) * source.height });
    const to = (u: number, v: number) => {
      const p = project(slice.corners, u, v);
      return { x: p.x * width, y: p.y * height };
    };
    for (let i = 0; i < STEPS; i++) {
      for (let j = 0; j < STEPS; j++) {
        const u0 = i / STEPS, u1 = (i + 1) / STEPS, v0 = j / STEPS, v1 = (j + 1) / STEPS;
        triangle(context, source, [from(u0, v0), from(u1, v0), from(u1, v1)], [to(u0, v0), to(u1, v0), to(u1, v1)]);
        triangle(context, source, [from(u0, v0), from(u1, v1), from(u0, v1)], [to(u0, v0), to(u1, v1), to(u0, v1)]);
      }
    }
  }
  context.fillStyle = "#000";
  for (const mask of masks) {
    if (mask.hidden || mask.points.length < 3) continue;
    context.beginPath();
    if (mask.invert) context.rect(0, 0, width, height);
    mask.points.forEach((p, i) => (i === 0 ? context.moveTo(p.x * width, p.y * height) : context.lineTo(p.x * width, p.y * height)));
    context.closePath();
    context.fill("evenodd");
  }
}
