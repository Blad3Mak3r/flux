import type { ReactNode } from "react";
import { PlusIcon, RefreshIcon, TrashIcon } from "../components/icons";
import { SelectField, TextField, TimecodeField } from "../components/fields";
import type { Devices } from "../devices/useDevices";
import { LOCALES, useI18n, useT } from "../i18n";
import type { Locale, MessageKey } from "../i18n";
import type { FrameRate, OutputConfig } from "../project/types";
import { edit } from "../state/actions";
import { setName, setOutput } from "../state/edit";
import { addDestination, removeDestination, updateDestination } from "../state/cues";
import { setOutputOffset } from "../state/output";
import { useDoc, useTimeline } from "../state/store";
import { renameTimeline, setTimecode } from "../state/timelines";
import { reconnect } from "../transport/playback";
import { useTransport } from "../transport/store";
import {
  addLtc,
  addMtc,
  clearMetronome,
  clearPreview,
  DEFAULT_LTC_LEVEL,
  isConnected,
  portChoices,
  portKey,
  portLabel,
  removeLtc,
  removeMtc,
  setDriver,
  setMetronomeChannels,
  setMetronomeDevice,
  setMetronomeGain,
  setMtcPort,
  setPreviewDevice,
  setProgrammeDevice,
  updateLtc,
} from "./outputs";

const FRAME_RATES: FrameRate[] = ["fps24", "fps25", "fps2997Drop", "fps30"];

type LampState = "off" | "on" | "fault";

/** A state lamp with its word beside it: the state never rides on colour alone. */
function RowStatus({ state, word }: { state: LampState; word: MessageKey }) {
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

function useOutput(): [OutputConfig, (next: OutputConfig) => void] {
  const project = useDoc((doc) => doc.project);
  return [project.output, (next) => edit(setOutput(project, next))];
}

/** The lamp for one row: running, missing or not yet connected, from what the output reported. */
function rowState(
  reported: { ok: boolean } | undefined,
  connected: boolean,
): { state: LampState; word: MessageKey } {
  if (!connected) return { state: "fault", word: "output.state.missing" };
  if (!reported) return { state: "off", word: "setup.state.waiting" };
  return reported.ok
    ? { state: "on", word: "output.state.running" }
    : { state: "fault", word: "output.state.missing" };
}

export function ProjectPage() {
  const t = useT();
  const project = useDoc((doc) => doc.project);
  return (
    <div className="setup-fields">
      <TextField label={t("project.name")} value={project.name} onCommit={(name) => edit(setName(project, name))} />
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

/** The options of a device list: the system default, a stored name that is not connected, the devices. */
function DeviceOptions({ current, devices }: { current: string; devices: Devices }) {
  const t = useT();
  return (
    <>
      <option value="">{t("output.default")}</option>
      {!isConnected(current, devices.audio.map((d) => d.name)) && (
        <option value={current}>{t("out.notConnected", { name: current })}</option>
      )}
      {devices.audio.map((d) => (
        <option key={d.name} value={d.name}>
          {d.name}
        </option>
      ))}
    </>
  );
}

const channelsOf = (devices: Devices, name: string, fallback: number) =>
  devices.audio.find((d) => d.name === name)?.outputChannels ?? fallback;

/** The first pair stereo interfaces offer, then each following one; a mono device offers channel 1. */
const pairsOf = (count: number): number[][] =>
  count < 2 ? [[1]] : Array.from({ length: Math.floor(count / 2) }, (_, i) => [2 * i + 1, 2 * i + 2]);

/** The select value that stands for "no preview device"; an empty name is the system default. */
const NO_PREVIEW = "__none__";

export function AudioPage({ devices }: { devices: Devices }) {
  const t = useT();
  const [output, change] = useOutput();
  const deviceName = output.audio?.deviceName ?? output.ltc[0]?.deviceName ?? "";
  const defaultHost = devices.hosts.find((host) => host.isDefault)?.name ?? "";
  const driver = output.driver ?? defaultHost;
  // A project from another system can name a driver this one lacks: it plays on the default one.
  const driverFound = !devices.available || devices.hosts.length === 0 || devices.hosts.some((host) => host.name === driver);
  const count = channelsOf(devices, deviceName, Math.max(...(output.audio?.channels ?? []), 2));
  const programme = output.audio?.channels.length ? output.audio.channels : [1, 2];
  const pairs = pairsOf(count);
  const preview = output.preview;
  const previewCount = channelsOf(devices, preview?.deviceName ?? "", Math.max(...(preview?.channels ?? []), 2));
  const previewChannels = preview?.channels.length ? preview.channels : [1, 2];
  return (
    <div className="setup-fields">
      {!devices.available && <p className="field-note">{t("out.desktopOnly")}</p>}
      {(devices.hosts.length > 1 || !driverFound) && (
        <SelectField
          label={t("out.driver")}
          value={driver}
          hint={t("setup.audio.driverNote")}
          disabled={!devices.available}
          onChange={(name) => change(setDriver(output, name === defaultHost ? undefined : name))}
        >
          {!driverFound && <option value={driver}>{t("out.driverMissing", { name: driver })}</option>}
          {devices.hosts.map((host) => (
            <option key={host.name} value={host.name}>
              {host.name}
            </option>
          ))}
        </SelectField>
      )}
      <SelectField
        label={t("out.audioDevice")}
        value={deviceName}
        hint={t("setup.audio.note")}
        disabled={!devices.available}
        onChange={(name) => change(setProgrammeDevice(output, name, channelsOf(devices, name, 2)))}
      >
        <DeviceOptions current={deviceName} devices={devices} />
      </SelectField>
      {devices.available && devices.audio.length === 0 && <p className="field-note">{t("out.noDevices")}</p>}
      <SelectField
        label={t("out.programme")}
        value={programme.join("-")}
        disabled={!devices.available}
        onChange={(pair) => change({ ...output, audio: { deviceName, channels: pair.split("-").map(Number) } })}
      >
        {pairs.map((pair) => (
          <option key={pair.join("-")} value={pair.join("-")}>
            {pair.length === 1
              ? t("out.mono", { channel: pair[0] })
              : t("out.pair", { first: pair[0], second: pair[1] })}
          </option>
        ))}
      </SelectField>
      <SelectField
        label={t("out.previewDevice")}
        value={preview ? preview.deviceName : NO_PREVIEW}
        hint={t("setup.audio.previewNote")}
        disabled={!devices.available}
        onChange={(name) =>
          change(name === NO_PREVIEW ? clearPreview(output) : setPreviewDevice(output, name, channelsOf(devices, name, 2)))
        }
      >
        <option value={NO_PREVIEW}>{t("out.previewNone")}</option>
        <DeviceOptions current={preview?.deviceName ?? ""} devices={devices} />
      </SelectField>
      {preview && (
        <SelectField
          label={t("out.previewChannels")}
          value={previewChannels.join("-")}
          disabled={!devices.available}
          onChange={(pair) => change({ ...output, preview: { ...preview, channels: pair.split("-").map(Number) } })}
        >
          {pairsOf(previewCount).map((pair) => (
            <option key={pair.join("-")} value={pair.join("-")}>
              {pair.length === 1
                ? t("out.mono", { channel: pair[0] })
                : t("out.pair", { first: pair[0], second: pair[1] })}
            </option>
          ))}
        </SelectField>
      )}
    </div>
  );
}

/** The select value that stands for "no metronome output". */
const NO_METRONOME = "__none__";

/** The metronome's own device, channels and level. It follows the tempo of the active timeline. */
export function MetronomePage({ devices }: { devices: Devices }) {
  const t = useT();
  const [output, change] = useOutput();
  const info = useTransport((transport) => transport.info);
  const metronome = output.metronome;
  const count = channelsOf(devices, metronome?.deviceName ?? "", Math.max(...(metronome?.channels ?? []), 2));
  const channels = metronome?.channels.length ? metronome.channels : [1, 2];
  const connected = !metronome || !devices.available || isConnected(metronome.deviceName, devices.audio.map((d) => d.name));
  const state = !metronome
    ? undefined
    : !connected || info?.metronome?.ok === false
      ? { state: "fault" as const, word: "output.state.missing" as const }
      : info?.metronome
        ? { state: "on" as const, word: "output.state.running" as const }
        : { state: "off" as const, word: "setup.state.waiting" as const };
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.metronome.intro")}</p>
      {!devices.available && <p className="field-note">{t("out.desktopOnly")}</p>}
      <SelectField
        label={t("out.metronomeDevice")}
        value={metronome ? metronome.deviceName : NO_METRONOME}
        hint={t("setup.metronome.note")}
        disabled={!devices.available}
        onChange={(name) =>
          change(name === NO_METRONOME ? clearMetronome(output) : setMetronomeDevice(output, name, channelsOf(devices, name, 2)))
        }
      >
        <option value={NO_METRONOME}>{t("out.metronomeNone")}</option>
        <DeviceOptions current={metronome?.deviceName ?? ""} devices={devices} />
      </SelectField>
      {metronome && (
        <>
          <SelectField
            label={t("out.metronomeChannels")}
            value={channels.join("-")}
            disabled={!devices.available}
            onChange={(pair) => change(setMetronomeChannels(output, pair.split("-").map(Number)))}
          >
            {pairsOf(count).map((pair) => (
              <option key={pair.join("-")} value={pair.join("-")}>
                {pair.length === 1
                  ? t("out.mono", { channel: pair[0] })
                  : t("out.pair", { first: pair[0], second: pair[1] })}
              </option>
            ))}
          </SelectField>
          <TextField
            label={t("out.metronomeLevel")}
            value={String(metronome.gainDb ?? 0)}
            inputMode="decimal"
            monospace
            validate={(text) => (Number.isFinite(Number(text)) && text.trim() !== "" ? null : t("field.numberInvalid"))}
            onCommit={(text) => change(setMetronomeGain(output, Number(text)))}
          />
          {state && <RowStatus {...state} />}
        </>
      )}
    </div>
  );
}

/** Lets a row's select be read without a visible label: the row's name and the control's purpose speak for it. */
function RowSelect({
  label,
  value,
  onChange,
  children,
}: {
  label: string;
  value: string | number;
  onChange: (value: string) => void;
  children: ReactNode;
}) {
  return (
    <select aria-label={label} value={value} onChange={(event) => onChange(event.target.value)}>
      {children}
    </select>
  );
}

export function LtcPage({ devices }: { devices: Devices }) {
  const t = useT();
  const [output, change] = useOutput();
  const info = useTransport((transport) => transport.info);
  const programmeDevice = output.audio?.deviceName ?? output.ltc[0]?.deviceName ?? "";
  // What the output reported belongs to these rows only while it has one entry for each.
  const reported = info && info.ltc.length === output.ltc.length ? info.ltc : [];
  const names = devices.audio.map((d) => d.name);
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.ltc.intro")}</p>
      {output.ltc.length === 0 && <p className="empty-note">{t("setup.ltc.empty")}</p>}
      <ul className="setup-rows">
        {output.ltc.map((ltc, index) => {
          const channels = channelsOf(devices, ltc.deviceName, Math.max(ltc.channel, 2));
          const name = t("setup.ltc.name", { number: index + 1 });
          const connected = !devices.available || isConnected(ltc.deviceName, names);
          return (
            <OutputRow
              key={index}
              name={name}
              {...rowState(reported[index], connected)}
              onRemove={() => change(removeLtc(output, index))}
            >
              <RowSelect
                label={`${name}: ${t("out.ltcDevice")}`}
                value={ltc.deviceName}
                onChange={(deviceName) =>
                  change(
                    updateLtc(output, index, {
                      deviceName,
                      channel: Math.min(ltc.channel, channelsOf(devices, deviceName, 2)),
                    }),
                  )
                }
              >
                <DeviceOptions current={ltc.deviceName} devices={devices} />
              </RowSelect>
              <RowSelect
                label={`${name}: ${t("out.ltcChannel")}`}
                value={ltc.channel}
                onChange={(channel) => change(updateLtc(output, index, { channel: Number(channel) }))}
              >
                {Array.from({ length: channels }, (_, i) => i + 1).map((channel) => (
                  <option key={channel} value={channel}>
                    {t("output.channel", { channel })}
                  </option>
                ))}
              </RowSelect>
              <TextField
                label={`${name}: ${t("out.ltcLevel")}`}
                value={String(ltc.level ?? DEFAULT_LTC_LEVEL)}
                inputMode="decimal"
                monospace
                validate={(text) => {
                  const level = Number(text);
                  return text.trim() !== "" && level >= 0 && level <= 1 ? null : t("field.levelInvalid");
                }}
                onCommit={(text) => change(updateLtc(output, index, { level: Number(text) }))}
              />
              <TextField
                label={`${name}: ${t("out.offset")}`}
                value={String(ltc.offsetMs ?? 0)}
                inputMode="numeric"
                monospace
                onCommit={(text) => void setOutputOffset("ltc", index, text)}
              />
            </OutputRow>
          );
        })}
      </ul>
      <div className="button-row">
        <button
          type="button"
          className="button"
          disabled={!devices.available}
          onClick={() => change(addLtc(output, programmeDevice, channelsOf(devices, programmeDevice, 2)))}
        >
          <PlusIcon />
          {t("setup.ltc.add")}
        </button>
      </div>
    </div>
  );
}

export function MtcPage({ devices }: { devices: Devices }) {
  const t = useT();
  const [output, change] = useOutput();
  const info = useTransport((transport) => transport.info);
  const reported = info && info.mtc.length === output.mtc.length ? info.mtc : [];
  const ports = devices.midi.map((port) => port.name);
  const choices = portChoices(ports);
  const listed = choices.map(portKey);
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.mtc.intro")}</p>
      {devices.midiError && <p className="field-note">{t("out.midiUnavailable")}</p>}
      {output.mtc.length === 0 && <p className="empty-note">{t("setup.mtc.empty")}</p>}
      <ul className="setup-rows">
        {output.mtc.map((mtc, index) => {
          const name = t("setup.mtc.name", { number: index + 1 });
          // A row with no port chosen has nothing to be missing.
          const key = portKey(mtc);
          const connected = key === "" || listed.includes(key);
          const state = rowState(reported[index], connected);
          return (
            <OutputRow
              key={index}
              name={name}
              {...(mtc.portName === "" ? { state: "off" as const, word: "output.state.off" as const } : state)}
              onRemove={() => change(removeMtc(output, index))}
            >
              <RowSelect
                label={`${name}: ${t("out.mtcPort")}`}
                value={key}
                onChange={(picked) => change(setMtcPort(output, index, picked))}
              >
                <option value="">{t("out.choose")}</option>
                {key !== "" && !connected && (
                  <option value={key}>{t("out.notConnected", { name: portLabel(mtc, choices) })}</option>
                )}
                {choices.map((choice) => (
                  <option key={portKey(choice)} value={portKey(choice)}>
                    {portLabel(choice, choices)}
                  </option>
                ))}
              </RowSelect>
              <TextField
                label={`${name}: ${t("out.offset")}`}
                value={String(mtc.offsetMs ?? 0)}
                inputMode="numeric"
                monospace
                onCommit={(text) => void setOutputOffset("mtc", index, text)}
              />
            </OutputRow>
          );
        })}
      </ul>
      <div className="button-row">
        <button type="button" className="button" onClick={() => change(addMtc(output, ports))}>
          <PlusIcon />
          {t("setup.mtc.add")}
        </button>
      </div>
    </div>
  );
}

/** The OSC receivers the OSC tracks choose from. The backend adds, checks and removes them. */
export function OscPage() {
  const t = useT();
  const destinations = useDoc((doc) => doc.project.output.osc ?? []);
  const reported = useTransport((transport) => transport.info?.cues ?? []);
  const integer = (text: string) => (/^\s*\d+\s*$/.test(text) ? null : t("field.integerInvalid"));
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.osc.intro")}</p>
      {destinations.length === 0 && <p className="empty-note">{t("setup.osc.empty")}</p>}
      <ul className="setup-rows">
        {destinations.map((destination) => {
          const result = reported.find((cue) => cue.kind === "osc" && cue.target === destination.name);
          const save = (patch: Partial<typeof destination>) => {
            const next = { ...destination, ...patch };
            void updateDestination(destination.id, next.name, next.host, next.port);
          };
          return (
            <OutputRow
              key={destination.id}
              name={destination.name}
              {...rowState(result, true)}
              onRemove={() => void removeDestination(destination.id)}
            >
              <TextField label={`${destination.name}: ${t("setup.osc.name")}`} value={destination.name} onCommit={(name) => save({ name })} />
              <TextField label={`${destination.name}: ${t("setup.osc.host")}`} value={destination.host} monospace onCommit={(host) => save({ host })} />
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
          onClick={() => void addDestination(t("setup.osc.default", { number: destinations.length + 1 }), "127.0.0.1", 53000)}
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
  const { locale, setLocale } = useI18n();
  return (
    <div className="setup-fields">
      <SelectField label={t("menu.language")} value={locale} onChange={(next) => setLocale(next as Locale)}>
        {LOCALES.map((option) => (
          <option key={option.id} value={option.id}>
            {option.label}
          </option>
        ))}
      </SelectField>
      <div className="setup-refresh">
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
    </div>
  );
}
