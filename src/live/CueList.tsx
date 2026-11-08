import { useVoice } from "../voice/store";
import { useTransport } from "../transport/store";
import { usePreferences } from "../preferences/store";
import { memo, useEffect, useRef } from "react";
import { useT } from "../i18n";
import { seek } from "../transport/playback";
import { menuAtPointer } from "../state/ui";
import { traceStyle } from "../timeline/palette";
import { cueMenu } from "../components/menus";
import type { Cue, CueStatus } from "./api";
import { cueName } from "./names";
import { useLive } from "./store";

const COLUMNS = ["number", "status", "cue", "track", "time", "length"] as const;

interface RowProps {
  cue: Cue;
  status: CueStatus;
  /** How far a running clip has played, 0 to 1. */
  progress: number | undefined;
}

/**
 * One cue. It re-renders only when its own status or progress changes, so a long list costs no
 * more per tick than the one or two rows that moved.
 */
const CueRow = memo(function CueRow({ cue, status, progress }: RowProps) {
  const t = useT();
  const name = cueName(cue, t);
  const voice = useVoice((v) => v.views[cue.id]);
  const route = usePreferences((p) => p.saved.operator);
  const routeLost = useTransport((s) => s.position?.operatorLost || s.info?.operator?.ok === false);
  const style = {
    ...traceStyle(cue.color ?? undefined),
    ...(progress !== undefined && { "--progress": `${progress * 100}%` }),
  };
  return (
    <li>
      <button
        type="button"
        className="cue-row"
        data-status={status}
        data-kind={cue.kind}
        style={style}
        title={t("live.hint")}
        // Going to a cue is a deliberate act in a show: a single click only focuses, a double
        // click or Enter (a click with no pointer) moves the transport.
        onClick={(event) => event.detail !== 1 && seek(cue.frame)}
        onContextMenu={(event) => menuAtPointer(event, cueMenu(t, cue))}
      >
        <span className="cue-number measure">{cue.number}</span>
        <span className="cue-state">
          <span className="lamp" data-status={status} aria-hidden="true" />
          {t(`live.status.${status}`)}
        </span>
        <span className="cue-kind" data-kind={cue.kind}>
          <span className="visually-hidden">{t(`live.kind.${cue.kind}`)}</span>
        </span>
        <span className="cue-name" title={name}>
          {name}
          {cue.kind === "operator" && <span className="voice-live-state">{t(`voice.status.${voice?.status ?? "ungenerated"}`)}{voice?.beforeOrigin && ` · ${t("voice.positionInvalid")}`}{(!route || routeLost) && ` · ${t("voice.routeMissing")}`}</span>}
        </span>
        <span className="cue-track" title={cue.trackName}>
          {cue.trackName}
        </span>
        <span className="cue-time measure">{cue.timecode}</span>
        <span className="cue-length measure">{cue.duration ?? "—"}</span>
      </button>
    </li>
  );
});

export function CueList() {
  const t = useT();
  const cues = useLive((live) => live.cues);
  const statuses = useLive((live) => live.state?.statuses);
  const running = useLive((live) => live.state?.running);
  const follow = useLive((live) => live.state?.next?.cueId ?? live.state?.current ?? null);
  const scroller = useRef<HTMLDivElement>(null);

  // The show moves down the list: keep the cue on standby (or the last one at the end) in view.
  useEffect(() => {
    const rows = scroller.current;
    const done = rows?.querySelectorAll('[data-status="done"]');
    const row = rows?.querySelector('[data-status="next"]') ?? done?.[done.length - 1];
    // Keep following inside the cue list; scrolling ancestors can hide the prepared show and GO.
    if (rows && row) {
      const bounds = rows.getBoundingClientRect();
      const target = row.getBoundingClientRect();
      const header = rows.querySelector(".cue-head")?.getBoundingClientRect().height ?? 0;
      if (target.top < bounds.top + header) rows.scrollTop += target.top - bounds.top - header;
      else if (target.bottom > bounds.bottom) rows.scrollTop += target.bottom - bounds.bottom;
    }
  }, [follow]);

  return (
    <section className="live-cues" aria-labelledby="live-cues-legend">
      <div className="live-cues-bar">
        <h2 id="live-cues-legend" className="legend">
          {t("live.cues")}
        </h2>
        <p className="live-hint">{t("live.hint")}</p>
      </div>
      <div className="cue-scroll" ref={scroller}>
        <div className="cue-head" aria-hidden="true">
          {COLUMNS.map((column) => (
            <span key={column} className={`cue-${column}`}>
              {t(`live.col.${column}`)}
            </span>
          ))}
        </div>
        <ol className="cue-list">
          {cues.map((cue, index) => (
            <CueRow
              key={cue.id}
              cue={cue}
              status={statuses?.[index] ?? "pending"}
              progress={running?.find((item) => item.cueId === cue.id)?.progress}
            />
          ))}
        </ol>
      </div>
    </section>
  );
}
