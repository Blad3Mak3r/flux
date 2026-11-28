// What the pages of Preferences share: the row of an output with its lamp, its state, the scope line and the device options.

import type { ReactNode } from "react";
import { TrashIcon } from "../components/icons";
import type { Devices } from "../devices/useDevices";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { UNASSIGNED } from "../preferences/store";

type LampState = "off" | "on" | "fault";

/** A state lamp with its word beside it: the state never rides on colour alone. */
export function RowStatus({ state, word }: { state: LampState; word: MessageKey }) {
  const t = useT();
  return (
    <span className="row-status">
      <span className="lamp" data-state={state} aria-hidden="true" />
      <span className="lamp-status">{t(word)}</span>
    </span>
  );
}

/** One output of a list: its name and state on the first line, its choices below. */
export function OutputRow({
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

/** The lamp for one row: running, missing or not yet connected, from what the output reported. */
export function rowState(
  reported: { ok: boolean } | undefined,
  connected: boolean,
  lost = false,
): { state: LampState; word: MessageKey } {
  if (!connected) return { state: "fault", word: "output.state.missing" };
  if (!reported) return { state: "off", word: "setup.state.waiting" };
  if (reported.ok && lost) return { state: "fault", word: "output.state.lost" };
  return reported.ok
    ? { state: "on", word: "output.state.running" }
    : { state: "fault", word: "output.state.missing" };
}

export function Scope({ project = false }: { project?: boolean }) {
  const t = useT();
  return (
    <p className="setup-scope">
      {t(project ? "setup.scope.project" : "setup.scope.machine")}
    </p>
  );
}

/** The options of a device list, as a call: `Select` reads them as children, so they cannot hide in a component. */
export function deviceOptions(
  t: ReturnType<typeof useT>,
  current: string,
  devices: Devices,
  optional = true,
) {
  return (
    <>
      {optional && <option value={UNASSIGNED}>{t("setup.unassigned")}</option>}
      <option value="">{t("output.default")}</option>
      {current !== UNASSIGNED &&
        current !== "" &&
        !devices.audio.some((d) => d.name === current) && (
          <option value={current}>
            {t("out.notConnected", { name: current })}
          </option>
        )}
      {devices.audio.map((d) => (
        <option key={d.name} value={d.name}>
          {d.name}
        </option>
      ))}
    </>
  );
}
