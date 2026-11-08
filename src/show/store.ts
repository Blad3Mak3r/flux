import { create } from "zustand";
import type { ShowState } from "./types";
export const useShow = create<{ state: ShowState | null; preparing: boolean }>()(() => ({ state: null, preparing: false }));
