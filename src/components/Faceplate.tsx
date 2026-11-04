import type { Devices, Presence } from "../devices/useDevices";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { useDoc } from "../state/store";
import { formatFrames } from "../timecode";
import { pause, play, stop } from "../transport/playback";
import { selectFrame, selectMode, useTransport } from "../transport/store";
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

/** The timecode, at the pace the frames change. */
function Readout() {
  const t = useT();
  const { frameRate, start } = useDoc((doc) => doc.project.timecode);
  const frame = useTransport(selectFrame);
  const state = useTransport(selectMode);
  const timecode = formatFrames(frame, frameRate);
  return (
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
  );
}

/** Redrawn only when the transport changes state, not with every frame. */
function Keys() {
  const t = useT();
  const state = useTransport(selectMode);
  return (
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
          onClick={() => void stop()}
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
          onClick={() => void play()}
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
          onClick={() => void pause()}
        >
          <PauseIcon />
          <span>{t("transport.pause")}</span>
        </button>
      </div>
    </section>
  );
}

function Lamps({ devices }: { devices: Devices }) {
  const t = useT();
  const output = useDoc((doc) => doc.project.output);
  const info = useTransport((transport) => transport.info);
  const deviceLost = useTransport((transport) => transport.position?.deviceLost);
  const ltcLost = useTransport((transport) => transport.position?.ltcLost);
  const clockUnpaced = useTransport((transport) => transport.position?.clockUnpaced);
  const running = info !== null;
  const { ltc, mtc } = output;
  // An empty name is the system's default output, which is what plays when nothing was chosen.
  const deviceName = output.audio?.deviceName ?? ltc?.deviceName ?? "";
  // LTC has a stream of its own when it names another device than the one that plays the programme.
  const ltcDevice = ltc && ltc.deviceName !== deviceName ? ltc.deviceName || t("output.default") : null;
  const clockRow = clockUnpaced
    ? { state: "fault" as LampState, status: "output.state.fast" as MessageKey }
    : rowState(true, devices.audioPresence === "unset" ? "found" : devices.audioPresence, running, deviceLost);

  const rows: OutputRow[] = [
    {
      legend: "output.clock",
      ...clockRow,
      detail: deviceName || t("output.default"),
    },
    {
      legend: "output.ltc",
      ...rowState(ltc !== null, devices.ltcPresence, running && info?.ltcChannel != null, deviceLost || ltcLost),
      detail: ltc
        ? [t("output.channel", { channel: ltc.channel }), ltcDevice].filter(Boolean).join(" · ")
        : t("output.notSet"),
    },
    {
      legend: "output.mtc",
      ...rowState(mtc !== null, devices.mtcPresence, running && info?.mtcPort != null),
      detail: mtc?.portName ?? t("output.notSet"),
    },
  ];

  return (
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
  );
}

export function Faceplate({ devices }: { devices: Devices }) {
  return (
    <header className="faceplate">
      <Readout />
      <Keys />
      <Lamps devices={devices} />
    </header>
  );
}
