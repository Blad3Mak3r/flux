import { useT } from "../i18n";
import { toggleFollow, useUi } from "../state/ui";

/**
 * Whether the timeline scrolls with the playhead. Scrolling by hand while the show plays turns it
 * off, visibly, and pressing it takes the view back.
 */
export function FollowToggle() {
  const t = useT();
  const on = useUi((ui) => ui.follow && !ui.detached);
  return (
    <button
      type="button"
      className="follow-toggle"
      aria-pressed={on}
      title={t("timeline.follow.hint")}
      onClick={toggleFollow}
    >
      <span className="lamp" data-state={on ? "on" : "off"} aria-hidden="true" />
      {t("timeline.follow")}
    </button>
  );
}
