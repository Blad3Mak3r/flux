// The timecode and show-control output pages of Preferences: LTC, MTC, MIDI clock and OSC.

import { changePreferences } from "../preferences/store";
import { PlusIcon } from "../components/icons";
import { SelectField, TextField } from "../components/fields";
import type { Devices } from "../devices/useDevices";
import { useT } from "../i18n";
import type { OscDestination } from "../project/types";
import { addDestination, removeDestination, updateDestination } from "../state/cues";
import { signalEdit } from "../state/output";
import { routeEdit, UNASSIGNED, usePreferences } from "../preferences/store";
import { useDoc } from "../state/store";
import type { CueInfo } from "../transport/api";
import { isLost } from "../transport/lost";
import { useTransport } from "../transport/store";
import { portChoices, portKey, portLabel, splitPortKey } from "./outputs";
import { RowStatus, OutputRow, rowState, Scope, deviceOptions } from "./rows";

const NO_OSC_DESTINATIONS: OscDestination[] = [];

const NO_CUES: CueInfo[] = [];

export function LtcPage({ devices }: { devices: Devices }) {
  const t = useT();
  const signals = useDoc((d) => d.project.output.ltc);
  const routes = usePreferences((p) => p.saved.ltc);
  const info = useTransport((p) => p.info);
  const lostMask = useTransport((p) => p.position?.ltcLost ?? 0);
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.ltc.intro")}</p>
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
                {deviceOptions(t, current, devices)}
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
      <p className="setup-intro">{t("setup.mtc.intro")}</p>
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

/**
 * MIDI clock out, for a cue show and a VJ show alike: whether the project sends it (a switch of the
 * project) and the MIDI port it goes to (a choice of this computer). The backend sends it.
 */
export function MidiClockPage({ devices }: { devices: Devices }) {
  const t = useT();
  const vj = useDoc((d) => d.project.mode === "vj");
  const enabled = useDoc((d) => d.project.output.midiClock ?? false);
  const port = usePreferences((p) => p.saved.midiClock);
  const info = useTransport((p) => p.info?.midiClock);
  const choices = portChoices(devices.midi.map((p) => p.name));
  const key = port ? portKey(port) : UNASSIGNED;
  const connected = !port || choices.some((c) => portKey(c) === key);
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.midiClock.intro")}</p>
      <p className="field-note">{t(vj ? "setup.midiClock.vj" : "setup.midiClock.cue")}</p>
      {devices.midiError && <p className="field-note">{t("out.midiUnavailable")}</p>}
      <Scope project />
      <label className="setup-check">
        <input
          type="checkbox"
          checked={enabled}
          onChange={(e) => signalEdit({ op: "setMidiClock", enabled: e.target.checked })}
        />
        {t("setup.midiClock.enabled")}
      </label>
      <Scope />
      <p className="field-note">{t("setup.midiClock.portNote")}</p>
      <SelectField
        label={t("out.mtcPort")}
        value={key}
        disabled={!devices.available}
        onChange={(value) => {
          const [name, index] = splitPortKey(value);
          void changePreferences({
            op: "midiClock",
            port: value === UNASSIGNED ? null : name,
            index,
          }).catch(() => undefined);
        }}
      >
        <option value={UNASSIGNED}>{t("setup.unassigned")}</option>
        {port && !connected && <option value={key}>{t("out.notConnected", { name: port.portName })}</option>}
        {choices.map((c) => (
          <option key={portKey(c)} value={portKey(c)}>
            {portLabel(c, choices)}
          </option>
        ))}
      </SelectField>
      {enabled && port && (
        <RowStatus
          {...(rowState(info ?? undefined, connected))}
        />
      )}
    </div>
  );
}

/** The OSC receivers the OSC tracks choose from. The backend adds, checks and removes them. */
export function OscPage() {
  const t = useT();
  const destinations = useDoc(
    (doc) => doc.project.output.osc ?? NO_OSC_DESTINATIONS,
  );
  const reported = useTransport((transport) => transport.info?.cues ?? NO_CUES);
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
