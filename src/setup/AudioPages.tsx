// The audio pages of Preferences: the device, the outputs and the metronome.

import { audioRoutingEdit, AudioRoutingStatus } from "../audio/routing";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { changePreferences } from "../preferences/store";
import { useState } from "react";
import { PlusIcon, TrashIcon } from "../components/icons";
import { Field, SelectField, TextField } from "../components/fields";
import type { Devices } from "../devices/useDevices";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { errorMessage } from "../project/api";
import { openSetup, sayError } from "../state/ui";
import { signalEdit } from "../state/output";
import { routeEdit, UNASSIGNED, usePreferences } from "../preferences/store";
import { useDoc } from "../state/store";
import { useTransport } from "../transport/store";
import { RowStatus, rowState, Scope, deviceOptions } from "./rows";

export function AudioRoute({
  kind,
  outputId,
  deviceLabel,
  channelsLabel,
  devices,
}: {
  kind: "audio" | "preview" | "metronome" | "operator";
  outputId?: string;
  deviceLabel: MessageKey;
  channelsLabel: MessageKey;
  devices: Devices;
}) {
  const t = useT();
  const route = usePreferences((p) => outputId ? p.saved.audioRoutes[outputId] : p.saved[kind]);
  const change = (field: string, text: string) => outputId ? void changePreferences({ op: "audioRoute", id: outputId, field, text }).catch(() => undefined) : routeEdit(kind, field, text);
  const current = route?.deviceName ?? (kind === "audio" && !outputId ? "" : UNASSIGNED);
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
  const channelChoices = kind !== "preview" && kind !== "metronome" ? [...Array.from({ length: count }, (_, index) => [index + 1]), ...pairs.filter((p) => p.length === 2)] : pairs;
  return (
    <>
      <SelectField
        label={t(deviceLabel)}
        value={current}
        disabled={!devices.available}
        onChange={(text) => change("device", text)}
      >
        {deviceOptions(t, current, devices, kind !== "audio" || Boolean(outputId))}
      </SelectField>
      {route && (
        <SelectField
          label={t(channelsLabel)}
          value={route.channels.join("-")}
          disabled={!devices.available}
          onChange={(text) => change("channels", text)}
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
      <h4>{t("setup.audio.driver")}</h4>
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
      <p className="field-note">{t("setup.audio.driverNote")}</p>
      <h4>{t("routing.programme")}</h4>
      <p className="field-note">{t("setup.audio.note")}</p>
      <AudioRoute
        kind="audio"
        deviceLabel="out.audioDevice"
        channelsLabel="out.programme"
        devices={devices}
      />
      <h4>{t("setup.audio.preview")}</h4>
      <p className="field-note">{t("setup.audio.previewNote")}</p>
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

export function AudioOutputsPage({ devices }: { devices: Devices }) {
  const t = useT();
  const outputs = useDoc((d) => d.project.output.audio);
  const playing = useTransport((s) => s.position?.state === "playing");
  const choices = usePreferences((p) => p.choices);
  const [name, setName] = useState("");
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<string | null>(null);
  const remove = choices.find((c) => c.id === removing);
  return <div className="setup-fields">
    <p className="field-note">{t("routing.portability")}</p>
    <details><summary>{t("routing.how")}</summary><p className="field-note">{t("routing.preview")}</p><p className="field-note">{t("routing.clocks")}</p></details>
    <AudioRoutingStatus />
    <p className="field-note">{t("routing.testNote")}</p>
    <ul className="setup-rows">
      {outputs?.map((bus) => <li className="setup-row audio-output-row" key={bus.id}>
        <span className="setup-row-name">{bus.name}</span>
        <button type="button" className="icon-button" title={t("setup.remove", { name: bus.name })} aria-label={t("setup.remove", { name: bus.name })}
          onClick={() => setRemoving(bus.id)}><TrashIcon /></button>
        <div className="setup-row-fields audio-output-fields">
          <TextField label={t("routing.name")} value={bus.name} onCommit={(name) => void audioRoutingEdit({ op: "rename", id: bus.id, name })} />
          <AudioRoute kind="audio" outputId={bus.id} deviceLabel="out.audioDevice" channelsLabel="routing.channels" devices={devices} />
          <button type="button" className="button" disabled={playing} onClick={() => void import("@tauri-apps/api/core").then(({ invoke }) => invoke("voice_test_output", { outputId: bus.id })).catch((e) => sayError(errorMessage(e)))}>{t("routing.test")}</button>
        </div>
      </li>)}
    </ul>
    <form className="setup-fields" onSubmit={(event) => { event.preventDefault(); if (adding) return; setAdding(true); void audioRoutingEdit({ op: "add", name }).then((applied) => { if (applied) setName(""); }).finally(() => setAdding(false)); }}>
      <Field label={t("routing.name")}>{({ id }) => <input id={id} value={name} disabled={adding} onChange={(event) => setName(event.target.value)} />}</Field>
      <button type="submit" className="button" disabled={adding}><PlusIcon />{t("routing.add")}</button>
    </form>
    <h4>{t("routing.builtins")}</h4>
    <p className="field-note">{t("routing.builtinsNote")}</p>
    <div className="button-row">
      <button type="button" className="button" onClick={() => openSetup("audio")}>{t("routing.goProgramme")}</button>
      <button type="button" className="button" onClick={() => openSetup("operator")}>{t("routing.goOperator")}</button>
    </div>
    <ConfirmDialog open={removing !== null} title={t("routing.removeTitle")} body={t("routing.removeBody", { count: remove?.users ?? 0 })} detail={remove?.name ?? undefined} confirmLabel={t("routing.remove")}
      onCancel={() => setRemoving(null)} onConfirm={() => { if (removing) void audioRoutingEdit({ op: "remove", id: removing }); setRemoving(null); }} />
  </div>;
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
      <p className="setup-intro">{t("setup.metronome.intro")}</p>
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
      <p className="field-note">{t("setup.metronome.note")}</p>
      {click && route && (
        <RowStatus {...rowState(info ?? undefined, connected, lost)} />
      )}
    </div>
  );
}
