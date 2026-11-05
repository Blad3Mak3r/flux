import { useEffect } from "react";
import { useT } from "../i18n";
import { setView } from "../state/ui";
import { selectMode, useTransport } from "../transport/store";
import { CueList } from "./CueList";
import { bindLive } from "./feed";
import { NextCue } from "./NextCue";
import { Running } from "./Running";
import { useLive } from "./store";
import { ViewSwitch } from "./ViewSwitch";

/**
 * The timeline as it is run on stage. Everything on it comes from the backend (cues, their state,
 * the countdowns) and this only draws it; the transport and the timecode stay on the faceplate.
 */
export function Live() {
  const t = useT();
  const empty = useLive((live) => live.cues.length === 0);
  const playing = useTransport((transport) => selectMode(transport) === "playing");

  useEffect(bindLive, []);

  return (
    <section className="live" data-playing={playing} aria-labelledby="live-legend">
      <div className="screen-bar">
        <h2 id="live-legend" className="legend">
          {t("live.legend")}
        </h2>
        <ViewSwitch />
      </div>
      {empty ? (
        <div className="empty-state">
          <p className="empty-title">{t("live.empty.title")}</p>
          <p>{t("live.empty.body")}</p>
          <div className="button-row">
            <button type="button" className="button" onClick={() => setView("edit")}>
              {t("live.empty.action")}
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="live-top">
            <NextCue />
            <Running />
          </div>
          <CueList />
        </>
      )}
    </section>
  );
}
