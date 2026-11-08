import { useEffect, useRef } from "react";
import { useT } from "../i18n";
import { showAction } from "./api";
import { useShow } from "./store";

/** The prepared plan's entry order, including repeated timelines, aliases and omissions. */
export function RunningOrder() {
  const t = useT();
  const state = useShow((s) => s.state);
  const current = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLOListElement>(null);
  // Follow the current entry inside this list only: scrolling ancestors would hide the header and GO.
  useEffect(() => {
    const rows = list.current;
    const button = current.current;
    if (!rows || !button) return;
    const bounds = rows.getBoundingClientRect();
    const target = button.getBoundingClientRect();
    const margin = 8;
    if (target.top < bounds.top + margin) rows.scrollTop += target.top - bounds.top - margin;
    else if (target.bottom > bounds.bottom - margin) rows.scrollTop += target.bottom - bounds.bottom + margin;
  }, [state?.entryId]);
  if (!state) return null;
  const stopped = state.position.state === "stopped";
  return <nav className="show-running-order" aria-label={t("show.runningOrder")}>
    <div className="show-order-caption"><span>{t("show.runningOrder")}</span>{!stopped && <span>{t("show.stopToPrepare")}</span>}</div>
    <ol ref={list}>
      {state.runningOrder.map((entry) => {
        // Only the entries that matter now say a word; passed and upcoming rows read by position and tone.
        const flagged = entry.status === "current" || entry.status === "next" || entry.status === "omitted";
        const status = t(`show.entryStatus.${entry.status}`);
        return <li key={entry.id} data-status={entry.status}>
          <button type="button" ref={entry.status === "current" ? current : undefined}
            aria-current={entry.status === "current" ? "step" : undefined}
            aria-disabled={!stopped || entry.omitted}
            title={t(entry.omitted ? "show.omitted" : stopped ? "show.prepareEntry" : "show.stopToPrepare")}
            onClick={() => { if (stopped && !entry.omitted) void showAction({ type: "prepare", entryId: entry.id }); }}>
            <span className="setlist-number">{entry.number}</span>
            <span className="show-order-entry">
              <strong>{entry.name}</strong>
              {(flagged || entry.source) && <span>{flagged ? <>{status}{entry.source && <> · {entry.source}</>}</> : entry.source}</span>}
              {!flagged && <span className="visually-hidden">{status}</span>}
            </span>
          </button>
        </li>;
      })}
    </ol>
  </nav>;
}
