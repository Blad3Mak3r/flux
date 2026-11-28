// The pages of Preferences about the show and the app: the project, the timeline, general and graphics.

import { fail } from "../state/fail";
import { protectVoiceWork } from "../voice/store";
import { LegacyRoutes } from "../preferences/LegacyRoutes";
import { useEffect, useState } from "react";
import { RefreshIcon } from "../components/icons";
import { SelectField, TextField, TimecodeField } from "../components/fields";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { FrameRate } from "../project/types";
import { DEFAULT_GRAPHICS, graphicsGet, graphicsRestart, graphicsSet } from "../graphics/api";
import type { GraphicsApi, GraphicsPrefs, GraphicsState } from "../graphics/api";
import { projectSetAutosave } from "../project/api";
import { edit } from "../state/actions";
import { isDirty } from "../state/doc";
import { ask } from "../state/ui";
import { setName } from "../state/edit";
import { LanguageSelect } from "../i18n/LanguageSelect";
import { dispatch, useDoc, useTimeline } from "../state/store";
import { flushDocumentSync } from "../state/persist";
import { renameTimeline, setTimecode } from "../state/timelines";
import { isPlaying } from "../transport/store";
import { useTransport } from "../transport/store";
import { Scope } from "./rows";

const FRAME_RATES: FrameRate[] = ["fps24", "fps25", "fps2997Drop", "fps30"];

export function ProjectPage() {
  const t = useT();
  const project = useDoc((doc) => doc.project);
  const [busy, setBusy] = useState(false);
  const changeAutosave = async (enabled: boolean, intervalSeconds: string) => {
    setBusy(true);
    try {
      await flushDocumentSync();
      const state = await projectSetAutosave(useDoc.getState().project, enabled, intervalSeconds);
      dispatch({ type: "apply", ...state });
    } catch (err) { fail(err); }
    finally { setBusy(false); }
  };
  return (
    <div className="setup-fields">
      <TextField label={t("project.name")} value={project.name} onCommit={(name) => edit(setName(project, name))} />
      <h4>{t("setup.project.autosave")}</h4>
      <label className="check-row">
        <input type="checkbox" checked={project.autosave.enabled} disabled={busy}
          onChange={(event) => void changeAutosave(event.target.checked, String(project.autosave.intervalSeconds))} />
        <span>{t("autosave.enabled")}</span>
      </label>
      <SelectField label={t("autosave.interval")} value={String(project.autosave.intervalSeconds)}
        disabled={busy || !project.autosave.enabled} hint={t("autosave.note")}
        onChange={(value) => void changeAutosave(project.autosave.enabled, value)}>
        {[15, 30, 60].map((seconds) => <option key={seconds} value={seconds}>{t("autosave.seconds", { seconds })}</option>)}
      </SelectField>
      <LegacyRoutes />
    </div>
  );
}

/** The name, frame rate and start of the active timeline. The backend applies them and answers. */
export function TimelinePage() {
  const t = useT();
  const timeline = useTimeline();
  const { frameRate, start } = timeline.timecode;
  return (
    // Another timeline starts the fields over, so a half-typed value never lands on it.
    <div className="setup-fields" key={timeline.id}>
      <TextField
        label={t("timeline.name")}
        value={timeline.name}
        onCommit={(name) => void renameTimeline(timeline.id, name)}
      />
      <SelectField
        label={t("project.frameRate")}
        value={frameRate}
        hint={t("project.frameRateNote")}
        onChange={(rate) => void setTimecode(rate as FrameRate, start)}
      >
        {FRAME_RATES.map((rate) => (
          <option key={rate} value={rate}>
            {t(`frameRate.${rate}` as MessageKey)}
          </option>
        ))}
      </SelectField>
      <TimecodeField
        label={t("project.start")}
        frame={start}
        rate={frameRate}
        onCommit={(frame) => void setTimecode(frameRate, frame)}
      />
    </div>
  );
}

export function GeneralPage() {
  return (
    <div className="setup-fields">
      <Scope />
      <LanguageSelect />
    </div>
  );
}

/**
 * How the window is drawn: hardware acceleration and, where the system allows it, the graphics API.
 * The webview reads them when it starts, so a change is saved at once and applies after a restart.
 */
export function GraphicsPage() {
  const t = useT();
  const [state, setState] = useState<GraphicsState | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const running = useTransport((transport) => transport.info !== null);

  useEffect(() => {
    graphicsGet().then(setState, (err) => { setLoadFailed(true); fail(err); });
  }, []);

  if (!state)
    return (
      loadFailed ? <button type="button" className="button" onClick={() => {
        setLoadFailed(false);
        void graphicsGet().then(setState, (err) => { setLoadFailed(true); fail(err); });
      }}>{t("dialog.retry")}</button> : <p className="empty-note">{t("setup.graphics.loading")}</p>
    );

  const change = (prefs: GraphicsPrefs) => {
    graphicsSet(prefs).then(setState, (err) => fail(err));
  };
  // The restart drops the document and the output, so it asks first about work that was not saved.
  const restart = () => {
    const go = () => void protectVoiceWork().then((safe) => { if (safe) return flushDocumentSync().then(graphicsRestart); }).catch((err) => fail(err));
    if (isDirty(useDoc.getState())) ask(go);
    else go();
  };
  const { saved } = state;
  const choosable = state.apis.length > 1;
  const playing = running && isPlaying();

  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.graphics.intro")}</p>
      {!state.accelerationConfigurable && (
        <p className="field-note">{t("setup.graphics.fixed")}</p>
      )}
      {state.safeMode && (
        <p className="field-note" role="status">
          {t("setup.graphics.safe")}
        </p>
      )}
      {state.accelerationConfigurable && (
        <label className="check-row">
          <input
            type="checkbox"
            checked={saved.hardwareAcceleration}
            onChange={(event) =>
              change({ ...saved, hardwareAcceleration: event.target.checked })
            }
          />
          <span>{t("setup.graphics.acceleration")}</span>
        </label>
      )}
      {choosable && (
        <SelectField
          label={t("setup.graphics.api")}
          value={saved.api}
          disabled={!saved.hardwareAcceleration}
          hint={
            saved.api === "vulkan"
              ? t("setup.graphics.vulkanNote")
              : t("setup.graphics.apiNote")
          }
          onChange={(api) => change({ ...saved, api: api as GraphicsApi })}
        >
          {state.apis.map((api) => (
            <option key={api} value={api}>
              {t(`setup.graphics.api.${api}`)}
            </option>
          ))}
        </SelectField>
      )}
      {state.restartNeeded && (
        <div className="setup-refresh">
          <button
            type="button"
            className="button"
            disabled={playing}
            onClick={restart}
          >
            <RefreshIcon />
            {t("setup.graphics.restart")}
          </button>
          <p className="field-note" role="status">
            {playing
              ? t("setup.graphics.restartPlaying")
              : t("setup.graphics.restartNote")}
          </p>
        </div>
      )}
      {state.accelerationConfigurable && (
        <button
          type="button"
          className="button"
          disabled={
            saved.hardwareAcceleration ===
              DEFAULT_GRAPHICS.hardwareAcceleration &&
            saved.api === DEFAULT_GRAPHICS.api
          }
          onClick={() => change(DEFAULT_GRAPHICS)}
        >
          {t("setup.graphics.reset")}
        </button>
      )}
      <p className="field-note">{t("setup.graphics.safeHint")}</p>
    </div>
  );
}
