import { useT } from "../i18n";
import { traceStyle } from "../timeline/palette";
import { cueName } from "./names";
import { useLive } from "./store";

/** The clips that sound now, each with the time left and a bar for how far it has played. */
export function Running() {
  const t = useT();
  const running = useLive((live) => live.state?.running);
  const cues = useLive((live) => live.cues);
  const last = useLive((live) => live.cues.find((cue) => cue.id === live.state?.current));

  return (
    <section className="live-panel live-running" aria-labelledby="live-running-legend">
      <h2 id="live-running-legend" className="legend">
        {t("live.running")}
      </h2>
      {running && running.length > 0 ? (
        <ul className="live-running-list">
          {running.map((item) => {
            const cue = cues.find((candidate) => candidate.id === item.cueId);
            if (!cue) return null;
            const name = cueName(cue, t);
            return (
              <li key={item.cueId} className="live-run" style={traceStyle(cue.color ?? undefined)}>
                <span className="live-number">{cue.number}</span>
                <p className="live-name" title={name}>
                  {name}
                </p>
                <span className="live-left measure">{item.remaining}</span>
                <div
                  className="live-bar"
                  role="progressbar"
                  aria-label={t("live.progress", { name })}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={Math.round(item.progress * 100)}
                >
                  <div style={{ width: `${item.progress * 100}%` }} />
                </div>
              </li>
            );
          })}
        </ul>
      ) : (
        <div className="live-quiet">
          <p>{t("live.nothing")}</p>
          {last && (
            <p className="live-sub">
              {t("live.last")} · {last.number} {cueName(last, t)}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
