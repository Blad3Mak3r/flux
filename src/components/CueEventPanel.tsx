import { setFirstTurnOnly } from "../state/cues";
import { useEffect, useState } from "react";
import { listMidiPorts } from "../devices/api";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type {
  FrameRate,
  MidiEvent,
  MidiMessage,
  MscCommand,
  OscArg,
  OscDestination,
  OscEvent,
  Track,
} from "../project/types";
import { deleteSelection } from "../state/actions";
import { moveCueEvent, setMidiMessage, setMidiPort, setOscDestination, setOscMessage } from "../state/cues";
import { portChoices, portKey, portLabel, splitPortKey } from "../setup/outputs";
import { SelectField, TextField, TimecodeField } from "./fields";
import { PlusIcon, TrashIcon } from "./icons";

// These panels show an event and forward what the user types. The backend checks it (ranges,
// addresses, the order of the events) and answers with the project, or with the reason it refused.

type MidiType = MidiMessage["type"];
const MIDI_TYPES: MidiType[] = ["noteOn", "noteOff", "controlChange", "programChange", "msc", "sysEx"];
const MSC_COMMANDS: MscCommand[] = ["go", "stop", "resume"];
const OSC_TYPES: OscArg["type"][] = ["int", "float", "string", "bool"];

/** The message a type starts from when the user switches to it. */
function starting(type: MidiType, channel: number): MidiMessage {
  switch (type) {
    case "noteOn":
      return { type, channel, note: 60, velocity: 100 };
    case "noteOff":
      return { type, channel, note: 60 };
    case "controlChange":
      return { type, channel, controller: 1, value: 0 };
    case "programChange":
      return { type, channel, program: 0 };
    case "msc":
      return { type, deviceId: 127, format: 1, command: "go" };
    case "sysEx":
      return { type, data: [0x7f] };
  }
}

const hex = (data: number[]) => data.map((byte) => byte.toString(16).toUpperCase().padStart(2, "0")).join(" ");

/** A whole number typed into a field. */
function NumberField({
  label,
  value,
  hint,
  onCommit,
}: {
  label: string;
  value: number;
  hint?: string;
  onCommit: (value: number) => void;
}) {
  const t = useT();
  return (
    <TextField
      label={label}
      hint={hint}
      value={String(value)}
      inputMode="numeric"
      monospace
      validate={(text) => (/^\s*-?\d+\s*$/.test(text) ? null : t("field.integerInvalid"))}
      onCommit={(text) => onCommit(Number(text))}
    />
  );
}

function MidiFields({ message, rate, send }: { message: MidiMessage; rate: FrameRate; send: (m: MidiMessage) => void }) {
  const t = useT();
  switch (message.type) {
    case "noteOn":
    case "noteOff":
      return (
        <>
          <NumberField label={t("midi.channel")} value={message.channel} onCommit={(channel) => send({ ...message, channel })} />
          <NumberField label={t("midi.note")} value={message.note} onCommit={(note) => send({ ...message, note })} />
          {message.type === "noteOn" && (
            <>
              <NumberField
                label={t("midi.velocity")}
                value={message.velocity}
                onCommit={(velocity) => send({ ...message, velocity })}
              />
              <TimecodeField
                label={t("midi.length")}
                hint={t("midi.length.hint")}
                frame={message.length ?? 0}
                rate={rate}
                onCommit={(frames) => {
                  const { length: _previous, ...rest } = message;
                  send(frames > 0 ? { ...rest, length: frames } : rest);
                }}
              />
            </>
          )}
        </>
      );
    case "controlChange":
      return (
        <>
          <NumberField label={t("midi.channel")} value={message.channel} onCommit={(channel) => send({ ...message, channel })} />
          <NumberField
            label={t("midi.controller")}
            value={message.controller}
            onCommit={(controller) => send({ ...message, controller })}
          />
          <NumberField label={t("midi.value")} value={message.value} onCommit={(value) => send({ ...message, value })} />
        </>
      );
    case "programChange":
      return (
        <>
          <NumberField label={t("midi.channel")} value={message.channel} onCommit={(channel) => send({ ...message, channel })} />
          <NumberField label={t("midi.program")} value={message.program} onCommit={(program) => send({ ...message, program })} />
        </>
      );
    case "msc":
      return (
        <>
          <NumberField
            label={t("msc.device")}
            hint={t("msc.device.hint")}
            value={message.deviceId}
            onCommit={(deviceId) => send({ ...message, deviceId })}
          />
          <NumberField
            label={t("msc.format")}
            hint={t("msc.format.hint")}
            value={message.format}
            onCommit={(format) => send({ ...message, format })}
          />
          <SelectField
            label={t("msc.command")}
            value={message.command}
            onChange={(command) => send({ ...message, command: command as MscCommand })}
          >
            {MSC_COMMANDS.map((command) => (
              <option key={command} value={command}>
                {t(`msc.${command}`)}
              </option>
            ))}
          </SelectField>
          <TextField
            label={t("msc.cue")}
            hint={t("msc.cue.hint")}
            value={message.cue ?? ""}
            monospace
            onCommit={(text) => {
              const { cue: _cue, list: _list, ...rest } = message;
              send(text.trim() === "" ? rest : { ...rest, cue: text.trim(), ...(message.list ? { list: message.list } : {}) });
            }}
          />
          {message.cue !== undefined && (
            <TextField
              label={t("msc.list")}
              value={message.list ?? ""}
              monospace
              onCommit={(text) => {
                const { list: _list, ...rest } = message;
                send(text.trim() === "" ? rest : { ...rest, list: text.trim() });
              }}
            />
          )}
        </>
      );
    case "sysEx":
      return (
        <TextField
          label={t("sysex.data")}
          hint={t("sysex.data.hint")}
          value={hex(message.data)}
          monospace
          validate={(text) => (/^\s*([0-9a-fA-F]{1,2}\s*)+$/.test(text) ? null : t("sysex.invalid"))}
          onCommit={(text) =>
            send({ ...message, data: text.trim().split(/\s+/).map((byte) => Number.parseInt(byte, 16)) })
          }
        />
      );
  }
}

/** The MIDI port a track sends to, from the ports connected now. */
function PortField({ track, port }: { track: Track; port: { portName: string; portIndex?: number } | undefined }) {
  const t = useT();
  const [names, setNames] = useState<string[]>([]);
  useEffect(() => {
    let current = true;
    listMidiPorts().then(
      (ports) => current && setNames(ports.map((listed) => listed.name)),
      () => undefined,
    );
    return () => {
      current = false;
    };
  }, []);
  const choices = portChoices(names);
  const key = port ? portKey(port) : "";
  const listed = choices.some((choice) => portKey(choice) === key);
  return (
    <SelectField
      label={t("midi.port")}
      value={key}
      onChange={(picked) => {
        if (picked === "") return void setMidiPort(track.id, null);
        const [portName, portIndex] = splitPortKey(picked);
        void setMidiPort(track.id, portIndex > 0 ? { portName, portIndex } : { portName });
      }}
    >
      <option value="">{t("midi.port.none")}</option>
      {port && !listed && <option value={key}>{t("midi.port.missing", { name: port.portName })}</option>}
      {choices.map((choice) => (
        <option key={portKey(choice)} value={portKey(choice)}>
          {portLabel(choice, choices)}
        </option>
      ))}
    </SelectField>
  );
}

export function MidiEventPanel({ track, event, start, rate }: { track: Track; event: MidiEvent; start: number; rate: FrameRate }) {
  const t = useT();
  const { message } = event;
  const send = (next: MidiMessage) => void setMidiMessage(event.id, next);
  const channel = "channel" in message ? message.channel : 1;
  return (
    <div className="panel-body" key={event.id}>
      <h3>{t("event.title.midi")}</h3>
      <RepeatField event={event} />
      <PortField track={track} port={track.kind.type === "midi" ? track.kind.port : undefined} />
      <TimecodeField
        label={t("event.at")}
        frame={start + event.at}
        rate={rate}
        onCommit={(frame) => void moveCueEvent(event.id, Math.max(0, frame - start))}
      />
      <SelectField
        label={t("midi.type")}
        value={message.type}
        onChange={(type) => send(starting(type as MidiType, channel))}
      >
        {MIDI_TYPES.map((type) => (
          <option key={type} value={type}>
            {t(`midi.type.${type}` as MessageKey)}
          </option>
        ))}
      </SelectField>
      <MidiFields message={message} rate={rate} send={send} />
      <DeleteButton />
    </div>
  );
}

function DeleteButton() {
  const t = useT();
  return (
    <button type="button" className="button danger" onClick={deleteSelection}>
      <TrashIcon />
      {t("inspector.delete")}
    </button>
  );
}

const blank = (type: OscArg["type"]): OscArg => {
  switch (type) {
    case "int":
      return { type, value: 0 };
    case "float":
      return { type, value: 0 };
    case "string":
      return { type, value: "" };
    case "bool":
      return { type, value: true };
  }
};

function OscArgRow({ arg, number, onChange, onRemove }: { arg: OscArg; number: number; onChange: (arg: OscArg) => void; onRemove: () => void }) {
  const t = useT();
  return (
    <li className="osc-arg">
      <select
        aria-label={t("osc.arg.type", { number })}
        value={arg.type}
        onChange={(event) => onChange(blank(event.target.value as OscArg["type"]))}
      >
        {OSC_TYPES.map((type) => (
          <option key={type} value={type}>
            {t(`osc.type.${type}` as MessageKey)}
          </option>
        ))}
      </select>
      {arg.type === "bool" ? (
        <select
          aria-label={t("osc.arg.value", { number })}
          value={String(arg.value)}
          onChange={(event) => onChange({ type: "bool", value: event.target.value === "true" })}
        >
          <option value="true">{t("osc.bool.true")}</option>
          <option value="false">{t("osc.bool.false")}</option>
        </select>
      ) : (
        <input
          // The value is applied on blur or Enter, like the fields of the panel.
          key={String(arg.value)}
          aria-label={t("osc.arg.value", { number })}
          className="measure"
          defaultValue={String(arg.value)}
          inputMode={arg.type === "string" ? "text" : "decimal"}
          onBlur={(event) => {
            const text = event.target.value;
            if (arg.type === "string") return text === arg.value ? undefined : onChange({ type: "string", value: text });
            const value = Number(text.replace(",", "."));
            if (Number.isFinite(value) && value !== arg.value) onChange({ ...arg, value: arg.type === "int" ? Math.round(value) : value });
            else event.target.value = String(arg.value);
          }}
          onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
        />
      )}
      <button type="button" className="icon-button" aria-label={t("osc.arg.remove", { number })} title={t("osc.arg.remove", { number })} onClick={onRemove}>
        <TrashIcon />
      </button>
    </li>
  );
}

export function OscEventPanel({
  track,
  event,
  start,
  rate,
  destinations,
}: {
  track: Track;
  event: OscEvent;
  start: number;
  rate: FrameRate;
  destinations: OscDestination[];
}) {
  const t = useT();
  const args = event.args ?? [];
  const send = (address: string, next: OscArg[]) => void setOscMessage(event.id, address, next);
  const destination = track.kind.type === "osc" ? (track.kind.destination ?? "") : "";
  return (
    <div className="panel-body" key={event.id}>
      <h3>{t("event.title.osc")}</h3>
      <RepeatField event={event} />
      <SelectField
        label={t("osc.destination")}
        hint={t("osc.destination.hint")}
        value={destination}
        onChange={(picked) => void setOscDestination(track.id, picked === "" ? null : picked)}
      >
        <option value="">{t("osc.destination.none")}</option>
        {destinations.map((listed) => (
          <option key={listed.id} value={listed.id}>
            {listed.name}
          </option>
        ))}
      </SelectField>
      <TimecodeField
        label={t("event.at")}
        frame={start + event.at}
        rate={rate}
        onCommit={(frame) => void moveCueEvent(event.id, Math.max(0, frame - start))}
      />
      <TextField
        label={t("osc.address")}
        hint={t("osc.address.hint")}
        value={event.address}
        monospace
        onCommit={(address) => send(address.trim(), args)}
      />
      <fieldset className="osc-args">
        <legend>{t("osc.args")}</legend>
        {args.length === 0 && <p className="field-note">{t("osc.args.empty")}</p>}
        <ul>
          {args.map((arg, index) => (
            <OscArgRow
              // Arguments have no identity of their own; their place is their key.
              key={index}
              arg={arg}
              number={index + 1}
              onChange={(next) => send(event.address, args.map((other, at) => (at === index ? next : other)))}
              onRemove={() => send(event.address, args.filter((_, at) => at !== index))}
            />
          ))}
        </ul>
        <button type="button" className="button" onClick={() => send(event.address, [...args, blank("int")])}>
          <PlusIcon />
          {t("osc.arg.add")}
        </button>
      </fieldset>
      <DeleteButton />
    </div>
  );
}

function RepeatField({event}: {event: MidiEvent | OscEvent}) {
  const t = useT();
  return <label className="check-row"><input type="checkbox" checked={event.firstTurnOnly ?? false}
    onChange={(e) => void setFirstTurnOnly(event.id,e.target.checked)} /><span>{t("show.firstTurnOnly")}</span></label>;
}
