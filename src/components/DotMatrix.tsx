import { memo, useEffect, useState } from "react";

// A 5x7 dot-matrix face, drawn as square pixels that are never antialiased. The SVG is sized in
// whole multiples of `--cell` (see the stylesheet), so a pixel is always a whole number of screen
// pixels.
const DIGITS: Record<string, string[]> = {
  "0": ["01110", "10001", "10011", "10101", "11001", "10001", "01110"],
  "1": ["00100", "01100", "00100", "00100", "00100", "00100", "01110"],
  "2": ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
  "3": ["11110", "00001", "00001", "01110", "00001", "00001", "11110"],
  "4": ["00010", "00110", "01010", "10010", "11111", "00010", "00010"],
  "5": ["11111", "10000", "11110", "00001", "00001", "10001", "01110"],
  "6": ["00110", "01000", "10000", "11110", "10001", "10001", "01110"],
  "7": ["11111", "00001", "00010", "00100", "01000", "01000", "01000"],
  "8": ["01110", "10001", "10001", "01110", "10001", "10001", "01110"],
  "9": ["01110", "10001", "10001", "01111", "00001", "00010", "01100"],
  ":": ["0", "0", "1", "0", "1", "0", "0"],
  ";": ["0", "0", "1", "0", "1", "1", "0"],
};

const ROWS = 7;
/** Cell size in CSS pixels at 1x: 9 on a wide window, 7 on a narrow one. */
const CELL_WIDE = 9;
const CELL_NARROW = 7;
const NARROW_BELOW = 1180;

/**
 * A cell is a whole number of *device* pixels with a gap of at least one, whatever the display
 * scale, so on a 125% or 150% screen no lit pixel is ever blended with its neighbour.
 */
function useCell(): { css: number; pixel: number } {
  const measure = () => {
    const dpr = window.devicePixelRatio || 1;
    const base = window.innerWidth < NARROW_BELOW ? CELL_NARROW : CELL_WIDE;
    const device = Math.max(4, Math.round(base * dpr));
    const gap = Math.max(1, Math.round(device * 0.12));
    return { css: device / dpr, pixel: (device - gap) / device };
  };
  const [cell, setCell] = useState(measure);
  useEffect(() => {
    const onResize = () => setCell(measure());
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return cell;
}

function pixels(rows: string[], lit: boolean, size: number): string {
  let path = "";
  rows.forEach((row, y) => {
    [...row].forEach((on, x) => {
      if ((on === "1") === lit) path += `M${x} ${y}h${size}v${size}h-${size}z`;
    });
  });
  return path;
}

const Glyph = memo(function Glyph({ char, x, size }: { char: string; x: number; size: number }) {
  const rows = DIGITS[char];
  if (!rows) return null;
  return (
    <g transform={`translate(${x} 0)`}>
      <path className="dot-off" d={pixels(rows, false, size)} />
      <path className="dot-on" d={pixels(rows, true, size)} />
    </g>
  );
});

/** Width in cells of a string set in this face, with one blank cell between glyphs. */
function widthOf(text: string): number {
  return [...text].reduce((sum, char, index) => sum + (DIGITS[char]?.[0].length ?? 0) + (index ? 1 : 0), 0);
}

export function DotMatrix({ text, label }: { text: string; label: string }) {
  const cell = useCell();
  let x = 0;
  const glyphs = [...text].map((char, index) => {
    const at = x;
    x += (DIGITS[char]?.[0].length ?? 0) + 1;
    return <Glyph key={index} char={char} x={at} size={cell.pixel} />;
  });
  const width = widthOf(text);
  return (
    <svg
      className="dot-matrix"
      viewBox={`0 0 ${width} ${ROWS}`}
      style={{ "--cols": width, "--cell": `${cell.css}px` } as React.CSSProperties}
      role="img"
      aria-label={label}
      shapeRendering="crispEdges"
    >
      {glyphs}
    </svg>
  );
}
