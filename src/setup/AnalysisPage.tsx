// What the VJ grid's audio modulation listens to: the grid's own sound, an audio input and channel, or nothing. The
// choice is the machine's, kept by the backend in its preferences; the backend opens the input and reads the bands,
// and this page shows what it hears.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Field } from "../components/fields";
import { listAudioHosts, type AudioHost } from "../devices/api";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { changePreferences, usePreferences } from "../preferences/store";
import { vjAnalysisLevels } from "../vj/api";
import type { AnalysisLevels } from "../vj/api";
import { RowStatus } from "./rows";
import { Select } from "../components/Select";

interface Input {
  name: string;
  channels: number[];
}

type Kind = "grid" | "input" | "off";
const KINDS: Kind[] = ["grid", "input", "off"];
const BANDS = ["low", "mid", "high", "level"] as const;

/** What the analysis hears now, as bars, and whether it listens. */
function Hearing() {
  const t = useT();
  const [levels, setLevels] = useState<AnalysisLevels | null>(null);
  useEffect(() => {
    let stopped = false;
    const tick = () => {
      vjAnalysisLevels()
        .then((next) => !stopped && setLevels(next))
        .catch(() => undefined)
        .finally(() => !stopped && window.setTimeout(tick, 100));
    };
    tick();
    return () => {
      stopped = true;
    };
  }, []);
  const state = levels?.state ?? "off";
  const lamp = state === "listening" ? "on" : state === "off" ? "off" : "fault";
  return (
    <div className="setup-analysis-hear" role="status">
      <p className="field-note">
        <RowStatus state={lamp} word={`setup.analysis.state.${state}` as MessageKey} />
      </p>
      <div className="setup-analysis-bars" role="group" aria-label={t("setup.analysis.levels")}>
        {BANDS.map((band) => (
          <div key={band} className="setup-analysis-bar" data-band={band}>
            <span className="setup-analysis-track"><i style={{ width: `${Math.round((levels?.[band] ?? 0) * 100)}%` }} /></span>
            <span className="field-note">{t(`vj.mod.band.${band}` as MessageKey)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function AnalysisPage() {
  const t = useT();
  const source = usePreferences((s) => s.saved.analysis) ?? { kind: "grid" as const };
  const [hosts, setHosts] = useState<AudioHost[]>([]);
  const [inputs, setInputs] = useState<Input[] | null>(null);
  const [refresh, setRefresh] = useState(0);
  const input = source.kind === "input" ? source : null;
  const driver = input?.driver ?? null;
  const selected = inputs?.find((candidate) => candidate.name === input?.device);

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

  // The grid's own sound is what is heard when nothing is chosen. An input with no device yet listens to nothing.
  const choose = (next: Kind) => {
    const change = next === "grid" ? null : next === "off" ? { kind: "off" } : { kind: "input", driver, device: input?.device ?? "", channel: input?.channel ?? 1 };
    void changePreferences({ op: "analysis", source: change }).catch(() => undefined);
  };
  const pick = (device: string, channel: number, host = driver) =>
    void changePreferences({ op: "analysis", source: { kind: "input", driver: host, device, channel } }).catch(() => undefined);

  return (
    <div className="setup-fields">
      <p className="field-note">{t("setup.analysis.note")}</p>
      <Field label={t("setup.analysis.source")}>
        {({ id }) => (
          <Select id={id} value={source.kind} onChange={(value) => choose(value as Kind)}>
            {KINDS.map((kind) => (
              <option key={kind} value={kind}>{t(`setup.analysis.source.${kind}` as MessageKey)}</option>
            ))}
          </Select>
        )}
      </Field>
      <h4>{t("setup.analysis.source.input")}</h4>
      <Field label={t("voice.inputDriver")}>
        {({ id }) => (
          <Select id={id} value={driver ?? ""} onChange={(value) => pick(input?.device ?? "", 1, value || null)}>
            <option value="">{t("voice.systemDriver")}</option>
            {hosts.map((host) => (
              <option key={host.name} value={host.name}>{host.name}</option>
            ))}
          </Select>
        )}
      </Field>
      <Field label={t("setup.analysis.device")}>
        {({ id }) => (
          <Select id={id} value={input?.device ?? ""} disabled={inputs === null} onChange={(value) => pick(value, 1)}>
            <option value="">{t(inputs === null ? "voice.loadingInputs" : "setup.analysis.none")}</option>
            {input?.device && inputs && !selected && <option value={input.device}>{t("voice.inputMissing", { name: input.device })}</option>}
            {inputs?.map((candidate) => (
              <option key={candidate.name} value={candidate.name}>{candidate.name}</option>
            ))}
          </Select>
        )}
      </Field>
      <Field label={t("setup.analysis.channel")}>
        {({ id }) => (
          <Select id={id} value={input?.channel ?? 1} disabled={!selected} onChange={(value) => input && pick(input.device, Number(value))}>
            {(selected?.channels ?? [input?.channel ?? 1]).map((channel) => (
              <option key={channel} value={channel}>{channel}</option>
            ))}
          </Select>
        )}
      </Field>
      <Hearing />
      <div>
        <button type="button" className="button" onClick={() => setRefresh((r) => r + 1)}>{t("voice.refreshInputs")}</button>
      </div>
    </div>
  );
}
