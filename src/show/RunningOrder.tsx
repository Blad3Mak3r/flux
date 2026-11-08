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
  useEffect(() => {
    const row = list.current;
    const button = current.current;
    if (row && button) row.scrollLeft += button.getBoundingClientRect().left - row.getBoundingClientRect().left - 12;
  }, [state?.entryId]);
  if (!state) return null;
  const stopped = state.position.state === "stopped";
  return <nav className="show-running-order" aria-label={t("show.runningOrder")}>
    <div className="show-order-caption"><span>{t("show.runningOrder")}</span><span>{t(stopped ? "show.chooseStoppedEntry" : "show.stopToPrepare")}</span></div>
    <ol ref={list}>
      {state.runningOrder.map((entry) => <li key={entry.id} data-status={entry.status}>
        <button type="button" ref={entry.status === "current" ? current : undefined}
          aria-current={entry.status === "current" ? "step" : undefined}
          aria-disabled={!stopped || entry.omitted}
          title={t(entry.omitted ? "show.omitted" : stopped ? "show.prepareEntry" : "show.stopToPrepare")}
          onClick={() => { if (stopped && !entry.omitted) void showAction({ type: "prepare", entryId: entry.id }); }}>
          <span className="setlist-number">{entry.number}</span>
          <span className="show-order-entry"><strong>{entry.name}</strong><span>{t(`show.entryStatus.${entry.status}`)}{entry.source && <> · {entry.source}</>}</span></span>
        </button>
      </li>)}
    </ol>
  </nav>;
}
