import { useEffect, useRef, useState } from "react";
import { PlusIcon, TrashIcon } from "../components/icons";
import { SelectField, TextField } from "../components/fields";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { controlGet, controlLearn, controlSet, onLearned } from "../control/api";
import type { ActionType, ControlAction, ControlPrefs, ControlView, LinkState } from "../control/api";
import {
  ACTION_TYPES,
  OSC_ADDRESSES,
  acceptsValue,
  addBinding,
  defaultAction,
  isNumbered,
  removeBinding,
  setBindingAction,
  triggerLabel,
  withValue,
} from "../control/model";
import { errorMessage } from "../project/api";
import { RowStatus } from "./pages";
import { portChoices, portKey, portLabel, splitPortKey } from "./outputs";

/** The lamp of an input: its word always rides beside it. */
function linkLamp(link: LinkState): { state: "off" | "on" | "fault"; word: MessageKey } {
  switch (link.state) {
    case "listening":
      return { state: "on", word: "setup.control.state.listening" };
    case "missing":
      return { state: "fault", word: "output.state.missing" };
    case "error":
      return { state: "fault", word: "setup.control.state.error" };
    default:
      return { state: "off", word: "output.state.off" };
  }
}

const actionKey = (type: ActionType) => `setup.control.action.${type}` as MessageKey;
const valueOf = (action: ControlAction) => ("value" in action ? action.value : 1);

/** The picker of an action and, for the ones that take one, its number. */
function ActionFields({
  label,
  action,
  onChange,
}: {
  label: string;
  action: ControlAction;
  onChange: (action: ControlAction) => void;
}) {
  const t = useT();
  return (
    <>
      <select aria-label={label} value={action.type} onChange={(event) => onChange(defaultAction(event.target.value as ActionType))}>
        {ACTION_TYPES.map((type) => (
          <option key={type} value={type}>
            {t(actionKey(type))}
          </option>
        ))}
      </select>
      {isNumbered(action.type) && (
        <TextField
          label={`${label}: ${t(`setup.control.value.${action.type}` as MessageKey)}`}
          value={String(valueOf(action))}
          inputMode="numeric"
          monospace
          validate={(text) => (acceptsValue(action.type, Number(text)) ? null : t("setup.control.valueInvalid"))}
          onCommit={(text) => onChange(withValue(action, Number(text)))}
        />
      )}
    </>
  );
}

/**
 * Lets a MIDI controller (pads, buttons) and OSC (a Stream Deck) run the show. The choices are the
 * machine's, kept by the backend in the app's own folder, not in the project. The backend listens
 * and runs the actions; this page shows how each input is doing and forwards the user's choices.
 */
export function ControlPage() {
  const t = useT();
  const [view, setView] = useState<ControlView | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [learning, setLearning] = useState(false);
  const [draft, setDraft] = useState<ControlAction>({ type: "play" });
  // The learn effect outlives the renders it was made in, so it reads the latest through this.
  const latest = useRef({ view, draft });
  latest.current = { view, draft };

  const fail = (err: unknown) => setProblem(errorMessage(err));
  const save = (prefs: ControlPrefs) => {
    setProblem(null);
    controlSet(prefs).then(setView, fail);
  };

  useEffect(() => {
    controlGet().then(setView, fail);
  }, []);

  useEffect(() => {
    if (!learning) return;
    void controlLearn(true);
    const stop = onLearned((trigger) => {
      const { view: now, draft: action } = latest.current;
      setLearning(false);
      if (now) save(addBinding(now.prefs, { trigger, action }));
    });
    return () => {
      stop();
      void controlLearn(false);
    };
  }, [learning]);

  if (!view) return <p className="empty-note">{problem ?? t("setup.control.loading")}</p>;

  const { prefs } = view;
  const { midi, osc } = prefs;
  const midiLamp = linkLamp(view.midi);
  const oscLamp = linkLamp(view.osc);
  const choices = portChoices(view.inputs.map((port) => port.name));
  const key = portKey({ portName: midi.portName ?? "", portIndex: midi.portIndex });
  const listed = choices.map(portKey);
  const canLearn = view.midi.state === "listening";
  const detail = (link: LinkState) => (link.state === "error" ? link.detail : null);

  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.control.intro")}</p>

      <h4>{t("setup.control.midi")}</h4>
      {view.midiError && <p className="field-note">{t("out.midiUnavailable")}</p>}
      <label className="check-row">
        <input type="checkbox" checked={midi.enabled} onChange={(event) => save({ ...prefs, midi: { ...midi, enabled: event.target.checked } })} />
        <span>{t("setup.control.midi.enable")}</span>
      </label>
      <SelectField
        label={t("setup.control.midi.port")}
        value={key}
        onChange={(picked) => {
          const [portName, portIndex] = splitPortKey(picked);
          save({ ...prefs, midi: { ...midi, portName: portName === "" ? null : portName, portIndex } });
        }}
      >
        <option value="">{t("out.choose")}</option>
        {key !== "" && !listed.includes(key) && (
          <option value={key}>{t("out.notConnected", { name: midi.portName ?? "" })}</option>
        )}
        {choices.map((choice) => (
          <option key={portKey(choice)} value={portKey(choice)}>
            {portLabel(choice, choices)}
          </option>
        ))}
      </SelectField>
      <RowStatus {...midiLamp} />
      {detail(view.midi) && <p className="field-note" role="alert">{detail(view.midi)}</p>}
      {view.midi.state === "missing" && (
        <div className="button-row">
          <button type="button" className="button" onClick={() => save(prefs)}>
            {t("setup.control.retry")}
          </button>
        </div>
      )}

      <h4>{t("setup.control.bindings")}</h4>
      <p className="field-note">{t("setup.control.bindings.note")}</p>
      {midi.bindings.length === 0 && <p className="empty-note">{t("setup.control.bindings.empty")}</p>}
      <ul className="setup-rows">
        {midi.bindings.map((binding, index) => {
          const name = triggerLabel(binding.trigger);
          return (
            <li key={`${name}-${index}`} className="setup-row">
              <span className="setup-row-name">{name}</span>
              <span />
              <button
                type="button"
                className="icon-button"
                aria-label={t("setup.remove", { name })}
                title={t("setup.remove", { name })}
                onClick={() => save(removeBinding(prefs, index))}
              >
                <TrashIcon />
              </button>
              <div className="setup-row-fields">
                <ActionFields label={name} action={binding.action} onChange={(action) => save(setBindingAction(prefs, index, action))} />
              </div>
            </li>
          );
        })}
      </ul>
      <div className="setup-row-fields">
        <ActionFields label={t("setup.control.new")} action={draft} onChange={setDraft} />
      </div>
      <div className="button-row">
        {learning ? (
          <>
            <p role="status">{t("setup.control.learning")}</p>
            <button type="button" className="button" onClick={() => setLearning(false)}>
              {t("setup.control.cancel")}
            </button>
          </>
        ) : (
          <button type="button" className="button" disabled={!canLearn} onClick={() => setLearning(true)}>
            <PlusIcon />
            {t("setup.control.learn")}
          </button>
        )}
      </div>
      {!canLearn && <p className="field-note">{t("setup.control.learn.needsPort")}</p>}

      <h4>{t("setup.control.osc")}</h4>
      <label className="check-row">
        <input type="checkbox" checked={osc.enabled} onChange={(event) => save({ ...prefs, osc: { ...osc, enabled: event.target.checked } })} />
        <span>{t("setup.control.osc.enable")}</span>
      </label>
      <TextField
        label={t("setup.control.osc.port")}
        value={String(osc.port)}
        inputMode="numeric"
        monospace
        validate={(text) => (/^\s*\d+\s*$/.test(text) && Number(text) >= 1 && Number(text) <= 65535 ? null : t("setup.control.osc.portInvalid"))}
        onCommit={(text) => save({ ...prefs, osc: { ...osc, port: Number(text) } })}
      />
      <TextField label={t("show.oscAddress")} value={osc.bindAddress ?? "0.0.0.0"}
        onCommit={(bindAddress) => save({...prefs,osc:{...osc,bindAddress}})} />
      <label className="check-row"><input type="checkbox" checked={osc.tcpEnabled ?? true}
        onChange={(e) => save({...prefs,osc:{...osc,tcpEnabled:e.target.checked}})} /><span>{t("show.oscTcp")}</span></label>
      <p className="field-note">{t("show.oscNote")}</p>
      <RowStatus {...oscLamp} />
      {detail(view.osc) && <p className="field-note" role="alert">{detail(view.osc)}</p>}
      <p className="field-note">{t("setup.control.osc.note")}</p>
      <ul className="osc-addresses">
        {OSC_ADDRESSES.map(({ address, numbered }) => (
          <li key={address}>
            <code>{numbered ? `${address} <n>` : address}</code>
          </li>
        ))}
      </ul>

      {problem && (
        <p className="field-note" role="alert">
          {problem}
        </p>
      )}
    </div>
  );
}
