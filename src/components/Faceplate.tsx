import type { Devices, Presence } from "../devices/useDevices";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { Project } from "../project/types";
import { formatFrames } from "../timecode";
import type { Playback } from "../transport/playback";
import { positionStore } from "../transport/playback";
import { useStore } from "../store";
import { DotMatrix } from "./DotMatrix";
import { PauseIcon, PlayIcon, StopIcon } from "./icons";

type LampState = "off" | "on" | "fault";

function Lamp({ state }: { state: LampState }) {
  return <span className="lamp" data-state={state} aria-hidden="true" />;
}

interface OutputRow {
  legend: MessageKey;
  state: LampState;
  status: MessageKey;
  detail: string;
}

function rowState(configured: boolean, presence: Presence, running: boolean, lost = false): { state: LampState; status: MessageKey } {
  if (!configured) return { state: "off", status: "output.state.off" };
  if (presence === "missing") return { state: "fault", status: "output.state.missing" };
  if (lost) return { state: "fault", status: "output.state.lost" };
  if (running) return { state: "on", status: "output.state.running" };
  return presence === "unknown"
    ? { state: "off", status: "output.state.unknown" }
    : { state: "off", status: "output.state.off" };
}

export function Faceplate({
  project,
  playback,
  devices,
}: {
  project: Project;
  playback: Playback;
  devices: Devices;
}) {
  const t = useT();
  const position = useStore(positionStore);
  const { frameRate, start } = project.timecode;
  const state = position?.state ?? "stopped";
  const frame = position?.frame ?? playback.parked;
  const timecode = formatFrames(frame, frameRate);
  const running = playback.info !== null;
  const { ltc, mtc } = project.output;
  // An empty name is the system's default output, which is what plays when nothing was chosen.
  const deviceName = ltc?.deviceName ?? project.output.audio?.deviceName ?? "";
  const clockRow = position?.clockUnpaced
    ? { state: "fault" as LampState, status: "output.state.fast" as MessageKey }
    : rowState(true, devices.audioPresence === "unset" ? "found" : devices.audioPresence, running, position?.deviceLost);

  const rows: OutputRow[] = [
    {
      legend: "output.clock",
      ...clockRow,
      detail: deviceName || t("output.default"),
    },
    {
      legend: "output.ltc",
      ...rowState(ltc !== null, devices.audioPresence, running, position?.deviceLost),
      detail: ltc ? t("output.channel", { channel: ltc.channel }) : t("output.notSet"),
    },
    {
      legend: "output.mtc",
      ...rowState(mtc !== null, devices.mtcPresence, running && playback.info?.mtcPort != null),
      detail: mtc?.portName ?? t("output.notSet"),
    },
  ];

  return (
    <header className="faceplate">
      <section className="readout" aria-label={t("readout.label")}>
        <DotMatrix text={timecode} label={`${t("readout.label")} ${timecode}`} />
        <p className="readout-line">
          <span className="state-word" data-state={state} role="status">
            {t(`transport.state.${state}` as MessageKey)}
          </span>
          <span>{t(`frameRate.${frameRate}` as MessageKey)}</span>
          <span>{t("readout.start", { timecode: formatFrames(start, frameRate) })}</span>
        </p>
      </section>

      <section className="keys" aria-labelledby="transport-legend">
        <h2 id="transport-legend" className="legend">
          {t("transport.legend")}
        </h2>
        <div className="key-row">
          <button
            type="button"
            className="key"
            aria-pressed={state === "stopped"}
            data-lamp={state === "stopped" ? "on" : "off"}
            onClick={() => void playback.stop()}
          >
            <StopIcon />
            <span>{t("transport.stop")}</span>
          </button>
          <button
            type="button"
            className="key"
            data-tone="live"
            aria-pressed={state === "playing"}
            data-lamp={state === "playing" ? "on" : "off"}
            onClick={() => void playback.play()}
          >
            <PlayIcon />
            <span>{t("transport.play")}</span>
          </button>
          <button
            type="button"
            className="key"
            data-tone="hold"
            aria-pressed={state === "paused"}
            data-lamp={state === "paused" ? "on" : "off"}
            onClick={() => void playback.pause()}
          >
            <PauseIcon />
            <span>{t("transport.pause")}</span>
          </button>
        </div>
      </section>

      <section className="lamps" aria-labelledby="output-legend">
        <h2 id="output-legend" className="legend">
          {t("output.legend")}
        </h2>
        <ul>
          {rows.map((row) => (
            <li key={row.legend}>
              <Lamp state={row.state} />
              <span className="lamp-name">{t(row.legend)}</span>
              <span className="lamp-status">{t(row.status)}</span>
              <span className="lamp-detail" title={row.detail}>
                {row.detail}
              </span>
            </li>
          ))}
        </ul>
      </section>
    </header>
  );
}
