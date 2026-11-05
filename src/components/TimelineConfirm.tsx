import { useT } from "../i18n";
import { useDoc } from "../state/store";
import { removeTimeline } from "../state/timelines";
import { clearRemoving, useUi } from "../state/ui";
import { ConfirmDialog } from "./ConfirmDialog";

/** Asks before a timeline, with its tracks, is removed from the project. */
export function TimelineConfirm() {
  const t = useT();
  const id = useUi((ui) => ui.removing);
  const name = useDoc((doc) => doc.project.timelines.find((timeline) => timeline.id === id)?.name ?? "");
  return (
    <ConfirmDialog
      open={id !== null}
      title={t("timeline.confirm.title", { name })}
      body={t("timeline.confirm.body")}
      confirmLabel={t("timeline.delete")}
      onCancel={clearRemoving}
      onConfirm={() => {
        clearRemoving();
        if (id !== null) void removeTimeline(id);
      }}
    />
  );
}
