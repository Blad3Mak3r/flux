import { useT } from "../i18n";
import type { DmxChannel, DmxDestination, DmxScene, FrameRate, Track } from "../project/types";
import { deleteSelection } from "../state/actions";
import { moveCueEvent } from "../state/cues";
import { addDmxChannel, removeDmxChannel, setDmxChannel, setDmxDestination, setDmxScene } from "../state/dmx";
import { SelectField, TextField, TimecodeField } from "./fields";
import { PlusIcon, TrashIcon } from "./icons";

// The Inspector's view of a DMX scene. It shows what the scene holds and forwards what the user
// types; the backend checks the numbers (universes, channels, values), keeps the channels in order
// and answers with the project, or with the reason it refused.

/** A whole number applied on blur or Enter; it goes back to the shown value when the backend refuses. */
function Cell({ label, value, onCommit }: { label: string; value: number; onCommit: (value: number) => Promise<boolean> }) {
  return (
    <input
      // The channel's own numbers are its identity: a new answer renews the field.
      key={value}
      aria-label={label}
      className="measure"
      defaultValue={String(value)}
      inputMode="numeric"
      onBlur={(event) => {
        const field = event.target;
        const typed = Number(field.value);
        if (!Number.isInteger(typed) || typed === value) {
          field.value = String(value);
          return;
        }
        void onCommit(typed).then((applied) => {
          if (!applied) field.value = String(value);
        });
      }}
      onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
    />
  );
}

function ChannelRow({ scene, channel }: { scene: DmxScene; channel: DmxChannel }) {
  const t = useT();
  const place = { universe: channel.universe, channel: channel.channel };
  const label = (what: "universe" | "channel" | "value") =>
    t(`dmx.${what}.of`, { universe: channel.universe, channel: channel.channel });
  return (
    <li className="dmx-channel">
      <Cell label={label("universe")} value={channel.universe} onCommit={(universe) => setDmxChannel(scene.id, place, { ...place, universe, value: channel.value })} />
      <Cell label={label("channel")} value={channel.channel} onCommit={(next) => setDmxChannel(scene.id, place, { ...place, channel: next, value: channel.value })} />
      <Cell label={label("value")} value={channel.value} onCommit={(value) => setDmxChannel(scene.id, place, { ...place, value })} />
      <button
        type="button"
        className="icon-button"
        aria-label={t("dmx.channel.remove", { universe: channel.universe, channel: channel.channel })}
        title={t("dmx.channel.remove", { universe: channel.universe, channel: channel.channel })}
        onClick={() => void removeDmxChannel(scene.id, channel.universe, channel.channel)}
      >
        <TrashIcon />
      </button>
    </li>
  );
}

export function DmxScenePanel({
  track,
  scene,
  start,
  rate,
  destinations,
}: {
  track: Track;
  scene: DmxScene;
  start: number;
  rate: FrameRate;
  destinations: DmxDestination[];
}) {
  const t = useT();
  const name = scene.name ?? "";
  const fade = scene.fade ?? 0;
  const releaseOthers = scene.releaseOthers ?? false;
  const destination = track.kind.type === "dmx" ? (track.kind.destination ?? "") : "";
  return (
    <div className="panel-body" key={scene.id}>
      <h3>{t("dmx.title")}</h3>
      <SelectField
        label={t("dmx.destination")}
        hint={t("dmx.destination.hint")}
        value={destination}
        onChange={(picked) => void setDmxDestination(track.id, picked === "" ? null : picked)}
      >
        <option value="">{t("dmx.destination.none")}</option>
        {destinations.map((listed) => (
          <option key={listed.id} value={listed.id}>
            {listed.name}
          </option>
        ))}
      </SelectField>
      <TimecodeField
        label={t("event.at")}
        frame={start + scene.at}
        rate={rate}
        onCommit={(frame) => void moveCueEvent(scene.id, Math.max(0, frame - start))}
      />
      <TextField label={t("dmx.name")} value={name} onCommit={(text) => void setDmxScene(scene.id, text, fade, releaseOthers)} />
      <TimecodeField
        label={t("dmx.fade")}
        hint={t("dmx.fade.hint")}
        frame={fade}
        rate={rate}
        onCommit={(frames) => void setDmxScene(scene.id, name, frames, releaseOthers)}
      />
      <label className="check-row">
        <input type="checkbox" checked={releaseOthers} onChange={(event) => void setDmxScene(scene.id, name, fade, event.target.checked)} />
        <span>{t("dmx.release")}</span>
      </label>
      <fieldset className="dmx-channels">
        <legend>{t("dmx.channels")}</legend>
        {scene.channels.length === 0 ? (
          <p className="field-note">{t("dmx.channels.empty")}</p>
        ) : (
          <>
            <div className="dmx-channel dmx-channel-head" aria-hidden="true">
              <span>{t("dmx.universe")}</span>
              <span>{t("dmx.channel")}</span>
              <span>{t("dmx.value")}</span>
              <span />
            </div>
            <ul>
              {scene.channels.map((channel) => (
                <ChannelRow key={`${channel.universe}/${channel.channel}`} scene={scene} channel={channel} />
              ))}
            </ul>
          </>
        )}
        <button type="button" className="button" onClick={() => void addDmxChannel(scene.id)}>
          <PlusIcon />
          {t("dmx.channel.add")}
        </button>
      </fieldset>
      <button type="button" className="button danger" onClick={deleteSelection}>
        <TrashIcon />
        {t("inspector.delete")}
      </button>
    </div>
  );
}
