import { useT } from "../i18n";
import { traceStyle } from "../timeline/palette";
import { cueName } from "./names";
import { useLive } from "./store";

/** The cue on standby and the time left until it: the first thing to read in a show. */
export function NextCue() {
  const t = useT();
  const countdown = useLive((live) => live.state?.next ?? null);
  const cue = useLive((live) => live.cues.find((candidate) => candidate.id === countdown?.cueId));
  const ended = useLive((live) => live.state !== null && live.state.next === null && live.cues.length > 0);

  return (
    <section className="live-panel live-next" aria-labelledby="live-next-legend">
      <h2 id="live-next-legend" className="legend">
        {t("live.next")}
      </h2>
      {cue && countdown ? (
        <div className="live-next-body" style={traceStyle(cue.color ?? undefined)}>
          <div className="live-next-cue">
            <span className="live-number">{cue.number}</span>
            <p className="live-name" title={cueName(cue, t)}>
              {cueName(cue, t)}
            </p>
            <p className="live-sub">
              {cue.trackName} · {t("live.at", { time: cue.timecode })}
            </p>
          </div>
          <p className="live-countdown" role="timer" aria-live="off">
            <span className="live-countdown-label">{t("live.in")}</span>
            <span className="measure">{countdown.text}</span>
          </p>
        </div>
      ) : (
        <p className="live-quiet">{ended ? t("live.end") : "—"}</p>
      )}
    </section>
  );
}
