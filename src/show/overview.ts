import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { hasNativeWindow } from "../components/WindowControls";
import { errorMessage } from "../project/api";
import { useDoc } from "../state/store";
import { sayError } from "../state/ui";
import type { SetlistOverview } from "./types";

/** Refresh derived show-sheet information after edits, undo, redo or loading a project. */
export function useShowOverview() {
  const project = useDoc((d) => d.project);
  const [result, setResult] = useState<{ project: typeof project; lists: SetlistOverview[] } | null>(null);
  useEffect(() => {
    if (!hasNativeWindow()) return;
    let cancelled = false;
    void invoke<SetlistOverview[]>("show_overview", { project }).then((lists) => {
      if (!cancelled) setResult({ project, lists });
    }).catch((error: unknown) => { if (!cancelled) sayError(errorMessage(error)); });
    return () => { cancelled = true; };
  }, [project]);
  return { lists: result?.lists ?? [], pending: result?.project !== project };
}
