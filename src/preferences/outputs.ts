import { useEffect } from "react";
import { useDoc } from "../state/store";
import { resolveOutput, usePreferences } from "./store";

export function useOutputView() {
  const show = useDoc((d) => d.project.output);
  const revision = usePreferences((p) => p.saved.revision);
  const output = usePreferences((p) => p.output);
  useEffect(() => {
    void resolveOutput(show).catch(() => undefined);
  }, [show, revision]);
  return output;
}
