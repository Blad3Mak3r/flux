// A 5x7 dot-matrix face, the one the app's readout uses (src/components/DotMatrix.tsx), drawn as
// square pixels in cell units.

export const DIGITS: Record<string, string[]> = {
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
};

export const ROWS = 7;
/** Slightly under 1 so a gap shows between pixels. */
const PIXEL = 0.88;

/** The path of the pixels of `char` that are lit (`lit`) or unlit. */
export function pixels(char: string, lit: boolean): string {
  let path = "";
  DIGITS[char].forEach((row, y) => {
    [...row].forEach((on, x) => {
      if ((on === "1") === lit) path += `M${x} ${y}h${PIXEL}v${PIXEL}h-${PIXEL}z`;
    });
  });
  return path;
}

/** Where each character of an `hh:mm:ss:ff` string starts, and the total width, in cells. */
export function layout(text: string): { offsets: number[]; width: number } {
  let x = 0;
  const offsets = [...text].map((char) => {
    const at = x;
    x += DIGITS[char][0].length + 1;
    return at;
  });
  return { offsets, width: x - 1 };
}
