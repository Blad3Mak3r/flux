import { useEffect } from "react";
import { useDoc } from "../state/store";
import { resolveOutput, usePreferences } from "./store";

export function useOutputView() {
  const project = useDoc((d) => d.project);
  const active = useDoc((d) => d.active);
  const revision = usePreferences((p) => p.saved.revision);
  const output = usePreferences((p) => p.output);
  useEffect(() => {
    void resolveOutput(project).catch(() => undefined);
  }, [project, active, revision]);
  return output;
}
