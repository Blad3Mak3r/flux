import type { Devices, Presence } from "../devices/useDevices";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { SetupSection } from "../setup/outputs";
import { activeTimeline } from "../state/doc";
import { useDoc } from "../state/store";
import { openSetup } from "../state/ui";
import { formatFrames } from "../timecode";
import { pause, play, stop } from "../transport/playback";
import { isLost } from "../transport/lost";
import { selectFrame, selectMode, useTransport } from "../transport/store";
import { DotMatrix } from "./DotMatrix";
import { PauseIcon, PlayIcon, StopIcon } from "./icons";

type LampState = "off" | "on" | "fault";

function Lamp({ state }: { state: LampState }) {
  return <span className="lamp" data-state={state} aria-hidden="true" />;
}

interface OutputRow {
  legend: MessageKey;
  /** The page of Setup that changes what this row shows. */
  section: SetupSection;
  state: LampState;
  status: MessageKey;
  detail: string;
  /** The whole detail, when it is summarised in the row. */
  title?: string;
}

function rowState(
  configured: boolean,
  presence: Presence,
  running: boolean,
  lost = false,
  failed = false,
): { state: LampState; status: MessageKey } {
  if (!configured) return { state: "off", status: "output.state.off" };
  if (presence === "missing" || failed) return { state: "fault", status: "output.state.missing" };
  if (lost) return { state: "fault", status: "output.state.lost" };
  if (running) return { state: "on", status: "output.state.running" };
  return presence === "unknown"
    ? { state: "off", status: "output.state.unknown" }
    : { state: "off", status: "output.state.off" };
}

/** The timecode, at the pace the frames change. */
function Readout() {
  const t = useT();
  const { frameRate, start } = useDoc((doc) => activeTimeline(doc).timecode);
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
  const ltcLost = useTransport((transport) => transport.position?.ltcLost ?? 0);
  const mtcLost = useTransport((transport) => transport.position?.mtcLost ?? 0);
  const clockUnpaced = useTransport((transport) => transport.position?.clockUnpaced);
  const running = info !== null;
  const { ltc } = output;
  // An empty name is the system's default output, which is what plays when nothing was chosen.
  const deviceName = output.audio?.deviceName ?? ltc[0]?.deviceName ?? "";
  const clockRow = clockUnpaced
    ? { state: "fault" as LampState, status: "output.state.fast" as MessageKey }
    : rowState(true, devices.audioPresence === "unset" ? "found" : devices.audioPresence, running, deviceLost);

  // What the output reported belongs to the outputs only while it has an entry for each.
  const ltcInfo = info && info.ltc.length === ltc.length ? info.ltc : null;
  // A row with no port chosen is not an output yet.
  const ports = output.mtc.map((mtc, index) => ({ mtc, reported: info?.mtc.length === output.mtc.length ? info.mtc[index] : undefined })).filter(({ mtc }) => mtc.portName !== "");

  const ltcDetails = ltc.map((one, index) =>
    [
      t("output.channel", { channel: one.channel }),
      one.deviceName !== deviceName ? one.deviceName || t("output.default") : null,
      deviceLost || isLost(ltcLost, index) ? t("output.state.lost") : null,
    ]
      .filter(Boolean)
      .join(" · "),
  );
  // The bits of the mask count every row of the project, including those with no port yet.
  const mtcDetails = output.mtc
    .map((mtc, index) => (isLost(mtcLost, index) ? `${mtc.portName} · ${t("output.state.lost")}` : mtc.portName))
    .filter((_, index) => output.mtc[index].portName !== "");
  const summary = (details: string[]) => (details.length > 1 ? t("output.count", { count: details.length }) : (details[0] ?? t("output.notSet")));

  const rows: OutputRow[] = [
    {
      legend: "output.clock",
      section: "audio",
      ...clockRow,
      detail: deviceName || t("output.default"),
    },
    {
      legend: "output.ltc",
      section: "ltc",
      ...rowState(
        ltc.length > 0,
        devices.ltcPresence,
        ltcInfo !== null && ltcInfo.every((one) => one.ok),
        deviceLost || ltcLost !== 0,
        ltcInfo?.some((one) => !one.ok),
      ),
      detail: summary(ltcDetails),
      title: ltcDetails.length > 1 || ltcLost !== 0 || deviceLost ? ltcDetails.join("\n") : undefined,
    },
    {
      legend: "output.mtc",
      section: "mtc",
      ...rowState(
        ports.length > 0,
        devices.mtcPresence,
        ports.every(({ reported }) => reported?.ok === true),
        mtcLost !== 0,
        ports.some(({ reported }) => reported?.ok === false),
      ),
      detail: summary(mtcDetails),
      title: mtcDetails.length > 1 || mtcLost !== 0 ? mtcDetails.join("\n") : undefined,
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
            <button type="button" className="lamp-row" title={row.title ?? t("setup.title")} onClick={() => openSetup(row.section)}>
              <Lamp state={row.state} />
              <span className="lamp-name">{t(row.legend)}</span>
              <span className="lamp-status">{t(row.status)}</span>
              <span className="lamp-detail">{row.detail}</span>
            </button>
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
