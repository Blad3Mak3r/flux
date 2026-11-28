// Small number and text helpers the screens share. Erasable syntax only, so `node --test` can load it.

/** A whole number as two digits. */
export const pad2 = (n: number): string => String(n).padStart(2, "0");

/** Rounded to two decimals, for display. */
export const round2 = (value: number): number => Math.round(value * 100) / 100;

/** A number typed in a field, where a comma may stand for the decimal point. `NaN` when it is not one. */
export const parseDecimal = (text: string): number => Number(text.replace(",", "."));
