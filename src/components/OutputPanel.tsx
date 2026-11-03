import { useT } from "../i18n";
import type { Devices } from "../devices/useDevices";
import type { OutputConfig, Project } from "../project/types";
import { setOutput } from "../state/edit";
import { startOutput, stopOutput } from "../transport/playback";
import { useTransport } from "../transport/store";
import { SelectField } from "./fields";
import { RefreshIcon } from "./icons";

/** LTC goes on the highest channel up to 8, which interfaces conventionally keep for sync. */
const DEFAULT_LTC_CHANNEL_CAP = 8;

export function OutputPanel({
  project,
  devices,
  onEdit,
}: {
  project: Project;
  devices: Devices;
  onEdit: (project: Project) => void;
}) {
  const t = useT();
  const info = useTransport((transport) => transport.info);
  const clipWarnings = useTransport((transport) => transport.clipWarnings);
  const { output } = project;
  const deviceName = output.ltc?.deviceName ?? output.audio?.deviceName ?? "";
  const device = devices.audio.find((d) => d.name === deviceName);
  const channelCount = device?.outputChannels ?? Math.max(output.ltc?.channel ?? 2, 2);
  const change = (next: Partial<OutputConfig>) => onEdit(setOutput(project, { ...output, ...next }));

  // Where the music goes: a stereo pair of the interface, `[1, 2]` unless the project says otherwise.
  const programme = output.audio?.channels.length ? output.audio.channels : [1, 2];
  const pairs: number[][] =
    channelCount < 2
      ? [[1]]
      : Array.from({ length: Math.floor(channelCount / 2) }, (_, i) => [2 * i + 1, 2 * i + 2]);

  const chooseDevice = (name: string) => {
    const chosen = devices.audio.find((d) => d.name === name);
    const channels = chosen?.outputChannels ?? 2;
    change({
      audio: { deviceName: name, channels: channels >= 2 ? [1, 2] : [1] },
      ltc: output.ltc ? { deviceName: name, channel: Math.min(output.ltc.channel, channels) } : null,
    });
  };

  const toggleLtc = (on: boolean) =>
    change({
      ltc: on ? { deviceName, channel: Math.min(DEFAULT_LTC_CHANNEL_CAP, channelCount) } : null,
    });

  const midiHint = output.mtc?.portName ?? "";
  const midiKnown = devices.midi.some((p) => p.name === midiHint);

  return (
    <div className="panel-body">
      {!devices.available && <p className="field-note">{t("out.desktopOnly")}</p>}

      <SelectField label={t("out.audioDevice")} value={deviceName} onChange={chooseDevice} disabled={!devices.available}>
        <option value="">{t("output.default")}</option>
        {deviceName && !device && <option value={deviceName}>{t("out.notConnected", { name: deviceName })}</option>}
        {devices.audio.map((d) => (
          <option key={`${d.host}:${d.name}`} value={d.name}>
            {d.name} · {d.host} · {t("out.channels", { count: d.outputChannels })}
          </option>
        ))}
      </SelectField>
      {devices.available && devices.audio.length === 0 && <p className="field-note">{t("out.noDevices")}</p>}

      <SelectField
        label={t("out.programme")}
        value={programme.join("-")}
        onChange={(pair) =>
          change({ audio: { deviceName, channels: pair.split("-").map(Number) } })
        }
        disabled={!devices.available}
      >
        {pairs.map((pair) => (
          <option key={pair.join("-")} value={pair.join("-")}>
            {pair.length === 1
              ? t("out.mono", { channel: pair[0] })
              : t("out.pair", { first: pair[0], second: pair[1] })}
          </option>
        ))}
      </SelectField>

      <div className="check-row">
        <input
          id="ltc-enable"
          type="checkbox"
          checked={output.ltc !== null}
          onChange={(event) => toggleLtc(event.target.checked)}
        />
        <label htmlFor="ltc-enable">{t("out.ltcEnable")}</label>
      </div>
      {output.ltc && (
        <SelectField
          label={t("out.ltcChannel")}
          value={output.ltc.channel}
          onChange={(channel) => change({ ltc: { ...output.ltc!, channel: Number(channel) } })}
        >
          {Array.from({ length: channelCount }, (_, i) => i + 1).map((channel) => (
            <option key={channel} value={channel}>
              {channel}
            </option>
          ))}
        </SelectField>
      )}

      <div className="check-row">
        <input
          id="mtc-enable"
          type="checkbox"
          checked={output.mtc !== null}
          onChange={(event) =>
            change({ mtc: event.target.checked ? { portName: devices.midi[0]?.name ?? "" } : null })
          }
        />
        <label htmlFor="mtc-enable">{t("out.mtcEnable")}</label>
      </div>
      {output.mtc && (
        <SelectField
          label={t("out.mtcPort")}
          value={midiHint}
          onChange={(portName) => change({ mtc: { portName } })}
          hint={devices.midiError ? t("out.midiUnavailable") : undefined}
        >
          <option value="">{t("out.choose")}</option>
          {midiHint && !midiKnown && <option value={midiHint}>{t("out.notConnected", { name: midiHint })}</option>}
          {devices.midi.map((port) => (
            <option key={port.name} value={port.name}>
              {port.name}
            </option>
          ))}
        </SelectField>
      )}

      <div className="button-row">
        <button type="button" className="button" onClick={devices.refresh}>
          <RefreshIcon />
          {t("out.refresh")}
        </button>
        {info ? (
          <button type="button" className="button" onClick={() => void stopOutput()}>
            {t("out.stop")}
          </button>
        ) : (
          <button type="button" className="button primary" onClick={() => void startOutput()}>
            {t("out.start")}
          </button>
        )}
      </div>

      {info && (
        <div className="run-info" role="status">
          <p>{t("out.runningOn", { device: info.device, rate: info.sampleRate })}</p>
          {[...info.warnings, ...clipWarnings].map((warning) => (
            <p key={warning} className="warning">
              {warning}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}
