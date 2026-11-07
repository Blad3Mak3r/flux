import { freezeVoices, protectVoiceWork } from "../voice/store";
import { VoiceSelector } from "../voice/VoiceSelector";
import { LegacyRoutes } from "../preferences/LegacyRoutes";
import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { PlusIcon, RefreshIcon, TrashIcon } from "../components/icons";
import { SelectField, TextField, TimecodeField } from "../components/fields";
import type { Devices } from "../devices/useDevices";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { FrameRate, OscDestination } from "../project/types";
import {
  DEFAULT_GRAPHICS,
  graphicsGet,
  graphicsRestart,
  graphicsSet,
} from "../graphics/api";
import type {
  GraphicsApi,
  GraphicsPrefs,
  GraphicsState,
} from "../graphics/api";
import { errorMessage, projectSetAutosave } from "../project/api";
import { edit } from "../state/actions";
import { isDirty } from "../state/doc";
import { ask } from "../state/ui";
import { setName } from "../state/edit";
import {
  addDestination,
  removeDestination,
  updateDestination,
} from "../state/cues";
import { signalEdit } from "../state/output";
import { routeEdit, UNASSIGNED, usePreferences } from "../preferences/store";
import { LanguageSelect } from "../i18n/LanguageSelect";
import { dispatch, useDoc, useTimeline } from "../state/store";
import { flushDocumentSync } from "../state/persist";
import { renameTimeline, setTimecode } from "../state/timelines";
import { reconnect } from "../transport/playback";
import { isLost } from "../transport/lost";
import { isPlaying } from "../transport/store";
import { useTransport } from "../transport/store";
import { portChoices, portKey, portLabel } from "./outputs";

const FRAME_RATES: FrameRate[] = ["fps24", "fps25", "fps2997Drop", "fps30"];
const NO_OSC_DESTINATIONS: OscDestination[] = [];

type LampState = "off" | "on" | "fault";

/** A state lamp with its word beside it: the state never rides on colour alone. */
export function RowStatus({ state, word }: { state: LampState; word: MessageKey }) {
  const t = useT();
  return (
    <span className="row-status">
      <span className="lamp" data-state={state} aria-hidden="true" />
      <span className="lamp-status">{t(word)}</span>
    </span>
  );
}

/** One output of a list: its name and state on the first line, its choices below. */
function OutputRow({
  name,
  state,
  word,
  onRemove,
  children,
}: {
  name: string;
  state: LampState;
  word: MessageKey;
  onRemove: () => void;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <li className="setup-row">
      <span className="setup-row-name">{name}</span>
      <RowStatus state={state} word={word} />
      <button
        type="button"
        className="icon-button"
        aria-label={t("setup.remove", { name })}
        title={t("setup.remove", { name })}
        onClick={onRemove}
      >
        <TrashIcon />
      </button>
      <div className="setup-row-fields">{children}</div>
    </li>
  );
}

/** The lamp for one row: running, missing or not yet connected, from what the output reported. */
function rowState(
  reported: { ok: boolean } | undefined,
  connected: boolean,
  lost = false,
): { state: LampState; word: MessageKey } {
  if (!connected) return { state: "fault", word: "output.state.missing" };
  if (!reported) return { state: "off", word: "setup.state.waiting" };
  if (reported.ok && lost) return { state: "fault", word: "output.state.lost" };
  return reported.ok
    ? { state: "on", word: "output.state.running" }
    : { state: "fault", word: "output.state.missing" };
}

export function ProjectPage() {
  const t = useT();
  const project = useDoc((doc) => doc.project);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const changeAutosave = async (enabled: boolean, intervalSeconds: string) => {
    setBusy(true);
    setProblem(null);
    try {
      await flushDocumentSync();
      const state = await projectSetAutosave(useDoc.getState().project, enabled, intervalSeconds);
      dispatch({ type: "apply", ...state });
    } catch (err) { setProblem(errorMessage(err)); }
    finally { setBusy(false); }
  };
  return (
    <div className="setup-fields">
      <TextField label={t("project.name")} value={project.name} onCommit={(name) => edit(setName(project, name))} />
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
      {problem && <p className="message" data-tone="error" role="alert">{problem}</p>}
      <VoiceSelector scope="project" value={project.voice} />
      <button type="button" className="button" onClick={() => void freezeVoices()}>{t("voice.freeze")}</button>
      <p className="field-note">{t("voice.freezeNote")}</p>
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

function Scope({ project = false }: { project?: boolean }) {
  const t = useT();
  return (
    <p className="setup-scope">
      {t(project ? "setup.scope.project" : "setup.scope.machine")}
    </p>
  );
}

function DeviceOptions({
  current,
  devices,
  optional = true,
}: {
  current: string;
  devices: Devices;
  optional?: boolean;
}) {
  const t = useT();
  return (
    <>
      {optional && <option value={UNASSIGNED}>{t("setup.unassigned")}</option>}
      <option value="">{t("output.default")}</option>
      {current !== UNASSIGNED &&
        current !== "" &&
        !devices.audio.some((d) => d.name === current) && (
          <option value={current}>
            {t("out.notConnected", { name: current })}
          </option>
        )}
      {devices.audio.map((d) => (
        <option key={d.name} value={d.name}>
          {d.name}
        </option>
      ))}
    </>
  );
}

export function AudioRoute({
  kind,
  deviceLabel,
  channelsLabel,
  devices,
}: {
  kind: "audio" | "preview" | "metronome" | "operator";
  deviceLabel: MessageKey;
  channelsLabel: MessageKey;
  devices: Devices;
}) {
  const t = useT();
  const route = usePreferences((p) => p.saved[kind]);
  const current = route?.deviceName ?? (kind === "audio" ? "" : UNASSIGNED);
  const count =
    devices.audio.find(
      (d) => d.name === current || (current === "" && d.isDefault),
    )?.outputChannels ?? 2;
  const pairs =
    count < 2
      ? [[1]]
      : Array.from({ length: Math.floor(count / 2) }, (_, i) => [
          2 * i + 1,
          2 * i + 2,
        ]);
  const channelChoices = kind === "operator" ? [...Array.from({ length: count }, (_, index) => [index + 1]), ...pairs.filter((p) => p.length === 2)] : pairs;
  return (
    <>
      <SelectField
        label={t(deviceLabel)}
        value={current}
        disabled={!devices.available}
        onChange={(text) => routeEdit(kind, "device", text)}
      >
        <DeviceOptions
          current={current}
          devices={devices}
          optional={kind !== "audio"}
        />
      </SelectField>
      {route && (
        <SelectField
          label={t(channelsLabel)}
          value={route.channels.join("-")}
          disabled={!devices.available}
          onChange={(text) => routeEdit(kind, "channels", text)}
        >
          {!channelChoices.some((p) => p.join("-") === route.channels.join("-")) && (
            <option value={route.channels.join("-")}>
              {route.channels.join(" / ")}
            </option>
          )}
          {channelChoices.map((p) => (
            <option key={p.join("-")} value={p.join("-")}>
              {p.length === 1
                ? t("out.mono", { channel: p[0] })
                : t("out.pair", { first: p[0], second: p[1] })}
            </option>
          ))}
        </SelectField>
      )}
    </>
  );
}

export function AudioPage({ devices }: { devices: Devices }) {
  const t = useT();
  const driver = usePreferences((p) => p.saved.driver) ?? "";
  return (
    <div className="setup-fields">
      <Scope />
      <p className="field-note">{t("setup.scope.shared")}</p>
      <SelectField
        label={t("out.driver")}
        value={driver}
        disabled={!devices.available}
        onChange={(text) => routeEdit("driver", "driver", text)}
      >
        <option value="">{t("output.default")}</option>
        {driver && !devices.hosts.some((h) => h.name === driver) && (
          <option value={driver}>
            {t("out.driverMissing", { name: driver })}
          </option>
        )}
        {devices.hosts.map((h) => (
          <option key={h.name} value={h.name}>
            {h.name}
          </option>
        ))}
      </SelectField>
      <AudioRoute
        kind="audio"
        deviceLabel="out.audioDevice"
        channelsLabel="out.programme"
        devices={devices}
      />
      <AudioRoute
        kind="preview"
        deviceLabel="out.previewDevice"
        channelsLabel="out.previewChannels"
        devices={devices}
      />
      {!devices.available && (
        <p className="field-note">{t("out.desktopOnly")}</p>
      )}
    </div>
  );
}

export function MetronomePage({ devices }: { devices: Devices }) {
  const t = useT();
  const click = useDoc((d) => d.project.output.metronome);
  const route = usePreferences((p) => p.saved.metronome);
  const info = useTransport((p) => p.info?.metronome);
  const lost = useTransport((p) => p.position?.metronomeLost ?? false);
  const connected =
    Boolean(route) &&
    (!devices.available ||
      route?.deviceName === "" ||
      devices.audio.some((d) => d.name === route?.deviceName));
  return (
    <div className="setup-fields">
      <Scope project />
      <label className="setup-check">
        <input
          type="checkbox"
          checked={Boolean(click)}
          onChange={(e) =>
            signalEdit({ op: "setMetronome", enabled: e.target.checked })
          }
        />
        {t("setup.metronome.enabled")}
      </label>
      {click && (
        <TextField
          label={t("out.metronomeLevel")}
          value={String(click.gainDb)}
          inputMode="decimal"
          monospace
          onCommit={(text) => signalEdit({ op: "setMetronomeGain", text })}
        />
      )}
      <Scope />
      <p className="field-note">{t("setup.scope.shared")}</p>
      <AudioRoute
        kind="metronome"
        deviceLabel="out.metronomeDevice"
        channelsLabel="out.metronomeChannels"
        devices={devices}
      />
      {click && route && (
        <RowStatus {...rowState(info ?? undefined, connected, lost)} />
      )}
    </div>
  );
}

export function LtcPage({ devices }: { devices: Devices }) {
  const t = useT();
  const signals = useDoc((d) => d.project.output.ltc);
  const routes = usePreferences((p) => p.saved.ltc);
  const info = useTransport((p) => p.info);
  const lostMask = useTransport((p) => p.position?.ltcLost ?? 0);
  return (
    <div className="setup-fields">
      <p className="field-note">{t("setup.scope.shared")}</p>
      {signals.length === 0 && (
        <p className="empty-note">{t("setup.ltc.empty")}</p>
      )}
      <ul className="setup-rows">
        {signals.map((signal, index) => {
          const route = routes[signal.slot];
          const current = route?.deviceName ?? UNASSIGNED;
          const count =
            devices.audio.find(
              (d) => d.name === current || (current === "" && d.isDefault),
            )?.outputChannels ?? Math.max(route?.channel ?? 1, 2);
          const name = t("setup.ltc.name", { number: signal.slot });
          const connected =
            Boolean(route) &&
            (current === "" || devices.audio.some((d) => d.name === current));
          return (
            <OutputRow
              key={signal.slot}
              name={name}
              {...(!route
                ? { state: "off" as const, word: "setup.unassigned" as const }
                : rowState(
                    info?.ltc[index],
                    connected,
                    isLost(lostMask, index),
                  ))}
              onRemove={() =>
                signalEdit({ op: "removeLtc", slot: signal.slot })
              }
            >
              <Scope project />
              <TextField
                label={t("out.ltcLevel")}
                value={String(signal.level ?? 0.5)}
                inputMode="decimal"
                monospace
                onCommit={(text) =>
                  signalEdit({ op: "setLtcLevel", slot: signal.slot, text })
                }
              />
              <Scope />
              <SelectField
                label={t("out.ltcDevice")}
                value={current}
                disabled={!devices.available}
                onChange={(text) =>
                  routeEdit("ltc", "device", text, signal.slot)
                }
              >
                <DeviceOptions current={current} devices={devices} />
              </SelectField>
              {route && (
                <>
                  <SelectField
                    label={t("out.ltcChannel")}
                    value={String(route.channel)}
                    disabled={!devices.available}
                    onChange={(text) =>
                      routeEdit("ltc", "channel", text, signal.slot)
                    }
                  >
                    {route.channel > count && (
                      <option value={route.channel}>
                        {t("output.channel", { channel: route.channel })}
                      </option>
                    )}
                    {Array.from({ length: count }, (_, i) => i + 1).map(
                      (channel) => (
                        <option key={channel} value={channel}>
                          {t("output.channel", { channel })}
                        </option>
                      ),
                    )}
                  </SelectField>
                  <TextField
                    label={t("out.offset")}
                    value={String(route.offsetMs ?? 0)}
                    inputMode="numeric"
                    monospace
                    onCommit={(text) =>
                      routeEdit("ltc", "offset", text, signal.slot)
                    }
                  />
                </>
              )}
            </OutputRow>
          );
        })}
      </ul>
      <button
        type="button"
        className="button"
        onClick={() => signalEdit({ op: "addLtc" })}
      >
        <PlusIcon />
        {t("setup.ltc.add")}
      </button>
    </div>
  );
}

export function MtcPage({ devices }: { devices: Devices }) {
  const t = useT();
  const signals = useDoc((d) => d.project.output.mtc);
  const routes = usePreferences((p) => p.saved.mtc);
  const info = useTransport((p) => p.info);
  const choices = portChoices(devices.midi.map((p) => p.name));
  const lostMask = useTransport((p) => p.position?.mtcLost ?? 0);
  return (
    <div className="setup-fields">
      <p className="field-note">{t("setup.scope.shared")}</p>
      {devices.midiError && (
        <p className="field-note">{t("out.midiUnavailable")}</p>
      )}
      {signals.length === 0 && (
        <p className="empty-note">{t("setup.mtc.empty")}</p>
      )}
      <ul className="setup-rows">
        {signals.map((signal, index) => {
          const route = routes[signal.slot];
          const key = route ? portKey(route) : UNASSIGNED;
          const connected = choices.some((c) => portKey(c) === key);
          const name = t("setup.mtc.name", { number: signal.slot });
          return (
            <OutputRow
              key={signal.slot}
              name={name}
              {...(!route
                ? { state: "off" as const, word: "setup.unassigned" as const }
                : rowState(
                    info?.mtc[index],
                    connected,
                    isLost(lostMask, index),
                  ))}
              onRemove={() =>
                signalEdit({ op: "removeMtc", slot: signal.slot })
              }
            >
              <Scope />
              <SelectField
                label={t("out.mtcPort")}
                value={key}
                disabled={!devices.available}
                onChange={(text) => routeEdit("mtc", "port", text, signal.slot)}
              >
                <option value={UNASSIGNED}>{t("setup.unassigned")}</option>
                {route && !connected && (
                  <option value={key}>
                    {t("out.notConnected", { name: route.portName })}
                  </option>
                )}
                {choices.map((c) => (
                  <option key={portKey(c)} value={portKey(c)}>
                    {portLabel(c, choices)}
                  </option>
                ))}
              </SelectField>
              {route && (
                <TextField
                  label={t("out.offset")}
                  value={String(route.offsetMs ?? 0)}
                  inputMode="numeric"
                  monospace
                  onCommit={(text) =>
                    routeEdit("mtc", "offset", text, signal.slot)
                  }
                />
              )}
            </OutputRow>
          );
        })}
      </ul>
      <button
        type="button"
        className="button"
        onClick={() => signalEdit({ op: "addMtc" })}
      >
        <PlusIcon />
        {t("setup.mtc.add")}
      </button>
    </div>
  );
}

/** The OSC receivers the OSC tracks choose from. The backend adds, checks and removes them. */
export function OscPage() {
  const t = useT();
  const destinations = useDoc(
    (doc) => doc.project.output.osc ?? NO_OSC_DESTINATIONS,
  );
  const reported = useTransport((transport) => transport.info?.cues ?? []);
  const integer = (text: string) =>
    /^\s*\d+\s*$/.test(text) ? null : t("field.integerInvalid");
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.osc.intro")}</p>
      {destinations.length === 0 && (
        <p className="empty-note">{t("setup.osc.empty")}</p>
      )}
      <ul className="setup-rows">
        {destinations.map((destination) => {
          const result = reported.find(
            (cue) => cue.kind === "osc" && cue.target === destination.name,
          );
          const save = (patch: Partial<typeof destination>) => {
            const next = { ...destination, ...patch };
            void updateDestination(
              destination.id,
              next.name,
              next.host,
              next.port,
            );
          };
          return (
            <OutputRow
              key={destination.id}
              name={destination.name}
              {...rowState(result, true)}
              onRemove={() => void removeDestination(destination.id)}
            >
              <TextField
                label={`${destination.name}: ${t("setup.osc.name")}`}
                value={destination.name}
                onCommit={(name) => save({ name })}
              />
              <TextField
                label={`${destination.name}: ${t("setup.osc.host")}`}
                value={destination.host}
                monospace
                onCommit={(host) => save({ host })}
              />
              <TextField
                label={`${destination.name}: ${t("setup.osc.port")}`}
                value={String(destination.port)}
                inputMode="numeric"
                monospace
                validate={integer}
                onCommit={(text) => save({ port: Number(text) })}
              />
            </OutputRow>
          );
        })}
      </ul>
      <div className="button-row">
        <button
          type="button"
          className="button"
          onClick={() =>
            void addDestination(
              t("setup.osc.default", { number: destinations.length + 1 }),
              "127.0.0.1",
              53000,
            )
          }
        >
          <PlusIcon />
          {t("setup.osc.add")}
        </button>
      </div>
    </div>
  );
}

export function GeneralPage({ devices }: { devices: Devices }) {
  const t = useT();
  return (
    <div className="setup-fields">
      <Scope />
      <LanguageSelect />
      <button
        type="button"
        className="button"
        onClick={() => {
          devices.refresh();
          reconnect(true);
        }}
      >
        <RefreshIcon />
        {t("out.refresh")}
      </button>
      <p className="field-note">{t("setup.general.refreshNote")}</p>
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
  const [problem, setProblem] = useState<string | null>(null);
  const running = useTransport((transport) => transport.info !== null);

  useEffect(() => {
    graphicsGet().then(setState, (err) => setProblem(errorMessage(err)));
  }, []);

  if (!state)
    return (
      <p className="empty-note">{problem ?? t("setup.graphics.loading")}</p>
    );

  const change = (prefs: GraphicsPrefs) => {
    setProblem(null);
    graphicsSet(prefs).then(setState, (err) => setProblem(errorMessage(err)));
  };
  // The restart drops the document and the output, so it asks first about work that was not saved.
  const restart = () => {
    const go = () => void protectVoiceWork().then((safe) => { if (safe) return flushDocumentSync().then(graphicsRestart); }).catch((err) => setProblem(errorMessage(err)));
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
      {problem && (
        <p className="field-note" role="alert">
          {problem}
        </p>
      )}
    </div>
  );
}
