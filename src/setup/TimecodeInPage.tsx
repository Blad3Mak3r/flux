// Where an external timecode comes in, for the VJ clock to follow: an audio input and channel for
// LTC, a MIDI input for MTC. The choices are the machine's, kept by the backend in its
// preferences; the backend opens the input and decodes it, and this page shows what it reads.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Field } from "../components/fields";
import { listAudioHosts, type AudioHost } from "../devices/api";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { changePreferences, usePreferences } from "../preferences/store";
import { formatFrames } from "../timecode";
import { listTimecodeMidiInputs, useTimecodeIn } from "../vj/timecodeIn";
import type { TimecodeInSource } from "../vj/timecodeIn";
import { RowStatus } from "./pages";
import { Select } from "../components/Select";

interface Input {
  name: string;
  channels: number[];
}

/** What an input reads, in words and with a lamp. */
function Reading({ source }: { source: TimecodeInSource }) {
  const t = useT();
  const status = useTimecodeIn(source);
  if (!status || status.state === "unset") return null;
  const { state, reading } = status;
  const lamp = state === "listening" ? (reading?.running ? "on" : "off") : "fault";
  const word: MessageKey =
    state !== "listening" ? `vj.timecodeIn.state.${state}` : reading === null ? "vj.timecodeIn.waiting" : reading.running ? "vj.timecodeIn.running" : "vj.timecodeIn.stopped";
  return (
    <p className="field-note" role="status">
      <RowStatus state={lamp} word={word} />{" "}
      {reading && (
        <span className="mono">
          {formatFrames(reading.frame, reading.rate)} · {t(`frameRate.${reading.rate}` as MessageKey)}
        </span>
      )}
    </p>
  );
}

export function TimecodeInPage() {
  const t = useT();
  const ltc = usePreferences((s) => s.saved.ltcIn);
  const mtc = usePreferences((s) => s.saved.mtcIn);
  const [hosts, setHosts] = useState<AudioHost[]>([]);
  const [inputs, setInputs] = useState<Input[] | null>(null);
  const [ports, setPorts] = useState<string[] | null>(null);
  const [refresh, setRefresh] = useState(0);
  const driver = ltc?.driver ?? null;
  const selected = inputs?.find((input) => input.name === ltc?.device);

  useEffect(() => {
    let disposed = false;
    setInputs(null);
    void Promise.all([listAudioHosts(), invoke<Input[]>("voice_inputs", { driver })])
      .then(([hosts, inputs]) => {
        if (!disposed) {
          setHosts(hosts);
          setInputs(inputs);
        }
      })
      .catch(() => !disposed && setInputs([]));
    return () => {
      disposed = true;
    };
  }, [driver, refresh]);
  useEffect(() => {
    let disposed = false;
    setPorts(null);
    listTimecodeMidiInputs().then(
      (ports) => !disposed && setPorts(ports),
      () => !disposed && setPorts([]),
    );
    return () => {
      disposed = true;
    };
  }, [refresh]);

  const chooseLtc = (device: string, channel: number, host = driver) =>
    void changePreferences({ op: "ltcIn", input: device === "" ? null : { driver: host, device, channel } }).catch(() => undefined);

  return (
    <div className="setup-fields">
      <p className="field-note">{t("vj.timecodeIn.note")}</p>
      <h4>{t("vj.timecodeIn.ltc")}</h4>
      <Field label={t("voice.inputDriver")}>
        {({ id }) => (
          <Select id={id} value={driver ?? ""} onChange={(value) => chooseLtc(ltc?.device ?? "", 1, value || null)}>
            <option value="">{t("voice.systemDriver")}</option>
            {hosts.map((host) => (
              <option key={host.name} value={host.name}>
                {host.name}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <Field label={t("vj.timecodeIn.ltcDevice")}>
        {({ id }) => (
          <Select id={id} value={ltc?.device ?? ""} disabled={inputs === null} onChange={(value) => chooseLtc(value, 1)}>
            <option value="">{t(inputs === null ? "voice.loadingInputs" : "vj.timecodeIn.none")}</option>
            {ltc?.device && inputs && !selected && <option value={ltc.device}>{t("voice.inputMissing", { name: ltc.device })}</option>}
            {inputs?.map((input) => (
              <option key={input.name} value={input.name}>
                {input.name}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <Field label={t("vj.timecodeIn.channel")}>
        {({ id }) => (
          <Select
            id={id}
            value={ltc?.channel ?? 1}
            disabled={!selected}
            onChange={(value) => ltc && chooseLtc(ltc.device, Number(value))}
          >
            {(selected?.channels ?? [ltc?.channel ?? 1]).map((channel) => (
              <option key={channel} value={channel}>
                {channel}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <Reading source="ltc" />

      <h4>{t("vj.timecodeIn.mtc")}</h4>
      <Field label={t("setup.control.midi.port")}>
        {({ id }) => (
          <Select
            id={id}
            value={mtc ?? ""}
            disabled={ports === null}
            onChange={(value) => void changePreferences({ op: "mtcIn", port: value || null }).catch(() => undefined)}
          >
            <option value="">{t(ports === null ? "voice.loadingInputs" : "vj.timecodeIn.none")}</option>
            {mtc && ports && !ports.includes(mtc) && <option value={mtc}>{t("out.notConnected", { name: mtc })}</option>}
            {ports?.map((port) => (
              <option key={port} value={port}>
                {port}
              </option>
            ))}
          </Select>
        )}
      </Field>
      <Reading source="mtc" />
      <div>
        <button type="button" className="button" onClick={() => setRefresh((r) => r + 1)}>
          {t("voice.refreshInputs")}
        </button>
      </div>
    </div>
  );
}
