import type { CSSProperties } from "react";
import type { VjGenerator, VjGeneratorKind } from "../project/types";

/** The generators the browser offers, in its order. */
export const GENERATORS: VjGeneratorKind[] = ["solid", "strobe", "gradient", "bars", "noise", "text"];

/** The colours of the bars, left to right, as the backend draws them. */
const BARS = ["#ffffff", "#ffff00", "#00ffff", "#00ff00", "#ff00ff", "#ff0000", "#0000ff"];

const NOISE = `url("data:image/svg+xml,${encodeURIComponent(
  "<svg xmlns='http://www.w3.org/2000/svg' width='160' height='90'><filter id='n'><feTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='2' stitchTiles='stitch'/><feColorMatrix type='saturate' values='0'/></filter><rect width='100%' height='100%' filter='url(#n)'/></svg>",
)}")`;

/** A text block's sketch: its letters in its colour on the dark of an empty composition. */
const sketch = (colour: string) =>
  `url("data:image/svg+xml,${encodeURIComponent(
    `<svg xmlns='http://www.w3.org/2000/svg' width='160' height='90'><text x='80' y='60' text-anchor='middle' font-family='sans-serif' font-weight='700' font-size='46' fill='${colour.replace(/[^#0-9a-fA-F]/g, "")}'>Aa</text></svg>`,
  )}")`;

/**
 * How a generator's thumbnail looks: a still sketch of what it draws, for the cells, the browser and the
 * inspector. A strobe is shown as stripes rather than flashing, so a screen full of them never blinks.
 * Parameters left out show as the backend's defaults.
 */
export function swatch(generator: VjGenerator): CSSProperties {
  switch (generator.kind) {
    case "solid":
      return { background: generator.colour ?? "#ffffff" };
    case "strobe": {
      const colour = generator.colour ?? "#ffffff";
      return { background: `repeating-linear-gradient(135deg, ${colour} 0 6px, #050607 6px 14px)` };
    }
    case "gradient":
      // CSS measures from the top, clockwise; the backend from the left towards the bottom.
      return {
        background: `linear-gradient(${(generator.angle ?? 0) + 90}deg, ${generator.from ?? "#000000"}, ${generator.to ?? "#ffffff"})`,
      };
    case "bars": {
      const stops = BARS.map((c, i) => `${c} ${(i / BARS.length) * 100}% ${((i + 1) / BARS.length) * 100}%`);
      return { background: `linear-gradient(90deg, ${stops.join(", ")})` };
    }
    case "text":
      return { backgroundColor: "#050607", backgroundImage: sketch(generator.colour ?? "#ffffff"), backgroundSize: "cover" };
    case "noise":
      return {
        backgroundColor: generator.colour ?? "#ffffff",
        backgroundImage: NOISE,
        backgroundSize: "cover",
        backgroundBlendMode: "multiply",
      };
  }
}
