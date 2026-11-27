// The switch that turns the output to one screen on or off, as the toggle in front of a screen in Resolume.

import { useT } from "../i18n";
import { useDoc } from "../state/store";
import { screenEdit } from "./mapping";

/** Whether screen `id` outputs; a screen the project does not list yet (the main one) does. */
export const useScreenEnabled = (id: string) =>
  useDoc((doc) => doc.project.output.screens?.find((s) => s.id === id)?.enabled !== false);

export function ScreenSwitch({ id, switchId }: { id: string; switchId?: string }) {
  const t = useT();
  const on = useScreenEnabled(id);
  const label = t("vj.ao.screenOutput");
  return (
    <button type="button" id={switchId} role="switch" aria-checked={on} className={`vj-sw${on ? " on" : ""}`} aria-label={label} title={label}
      onClick={() => void screenEdit({ op: "enable", id, enabled: !on })}>
      <i />
    </button>
  );
}
