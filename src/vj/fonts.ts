import { useEffect, useState } from "react";
import type { VjFont } from "../project/types";
import { vjFonts } from "./api";

let loaded: Promise<VjFont[]> | undefined;

/** The font families the backend lists, asked for once for the whole session; none until they arrive. */
export function useFonts(): VjFont[] {
  const [fonts, setFonts] = useState<VjFont[]>([]);
  useEffect(() => {
    let live = true;
    loaded ??= vjFonts().catch(() => {
      loaded = undefined;
      return [];
    });
    void loaded.then((list) => live && setFonts(list));
    return () => {
      live = false;
    };
  }, []);
  return fonts;
}
