import { useT } from "../i18n";
import type { Devices } from "../devices/useDevices";
import type { OutputConfig, Project } from "../project/types";
import { setOutput } from "../state/edit";
import type { Playback } from "../transport/playback";
import { SelectField } from "./fields";
import { RefreshIcon } from "./icons";

/** LTC goes on the highest channel up to 8, which interfaces conventionally keep for sync. */
const DEFAULT_LTC_CHANNEL_CAP = 8;

export function OutputPanel({
  project,
  devices,
  playback,
  onEdit,
}: {
  project: Project;
  devices: Devices;
  playback: Playback;
  onEdit: (project: Project) => void;
}) {
  const t = useT();
  const { output } = project;
  const deviceName = output.ltc?.deviceName ?? output.audio?.deviceName ?? "";
  const device = devices.audio.find((d) => d.name === deviceName);
  const channelCount = device?.outputChannels ?? Math.max(output.ltc?.channel ?? 2, 2);
  const change = (next: Partial<OutputConfig>) => onEdit(setOutput(project, { ...output, ...next }));

  const chooseDevice = (name: string) => {
    const chosen = devices.audio.find((d) => d.name === name);
    const channels = chosen?.outputChannels ?? 2;
    change({
      audio: name ? { deviceName: name, channels: channels >= 2 ? [1, 2] : [1] } : null,
      ltc: output.ltc && name
        ? { deviceName: name, channel: Math.min(output.ltc.channel, channels) }
        : null,
    });
  };

  const toggleLtc = (on: boolean) =>
    change({
      ltc: on && deviceName
        ? { deviceName, channel: Math.min(DEFAULT_LTC_CHANNEL_CAP, channelCount) }
        : null,
    });

  const midiHint = output.mtc?.portName ?? "";
  const midiKnown = devices.midi.some((p) => p.name === midiHint);

  return (
    <div className="panel-body">
      {!devices.available && <p className="field-note">{t("out.desktopOnly")}</p>}

      <SelectField label={t("out.audioDevice")} value={deviceName} onChange={chooseDevice} disabled={!devices.available}>
        <option value="">{t("out.choose")}</option>
        {deviceName && !device && <option value={deviceName}>{t("out.notConnected", { name: deviceName })}</option>}
        {devices.audio.map((d) => (
          <option key={`${d.host}:${d.name}`} value={d.name}>
            {d.name} · {d.host} · {t("out.channels", { count: d.outputChannels })}
          </option>
        ))}
      </SelectField>
      {devices.available && devices.audio.length === 0 && <p className="field-note">{t("out.noDevices")}</p>}

      <div className="check-row">
        <input
          id="ltc-enable"
          type="checkbox"
          checked={output.ltc !== null}
          disabled={!deviceName}
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
        {playback.info ? (
          <button type="button" className="button" onClick={() => void playback.stopOutput()}>
            {t("out.stop")}
          </button>
        ) : (
          <button type="button" className="button primary" onClick={() => void playback.startOutput()} disabled={!deviceName}>
            {t("out.start")}
          </button>
        )}
      </div>

      {playback.info && (
        <div className="run-info" role="status">
          <p>{t("out.runningOn", { device: playback.info.device, rate: playback.info.sampleRate })}</p>
          {playback.info.warnings.map((warning) => (
            <p key={warning} className="warning">
              {warning}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}
