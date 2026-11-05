import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { Project, Track, TriggerAction } from "../project/types";
import { deleteSelection, edit } from "../state/actions";
import type { AssetAudio } from "../state/doc";
import { useDoc } from "../state/store";
import { toggleInspector, useUi } from "../state/ui";
import type { Selected } from "../state/ui";
import { findItem, setTrackColor, updateClip, updateTrigger } from "../state/edit";
import { TRACK_COLORS } from "../timeline/palette";
import { SelectField, TextField, TimecodeField } from "./fields";
import { ChevronLeftIcon, ChevronRightIcon, TrashIcon } from "./icons";
import { MOD_KEY } from "./MenuBar";

const hex = (bytes: number[]) => bytes.map((b) => b.toString(16).toUpperCase().padStart(2, "0")).join(" ");

/** Parses "C0 05" into bytes, or `null` when it is not a list of hex bytes. */
function parseBytes(text: string): number[] | null {
  const parts = text.trim().split(/\s+/);
  if (parts.length === 0 || parts[0] === "") return null;
  const bytes = parts.map((part) => (/^[0-9a-f]{1,2}$/i.test(part) ? parseInt(part, 16) : NaN));
  return bytes.some(Number.isNaN) ? null : bytes;
}

/** The colour of the track an item sits on. Each swatch is named, so colour is never the only cue. */
function TrackColorField({
  project,
  track,
  onEdit,
}: {
  project: Project;
  track: Track;
  onEdit: (project: Project) => void;
}) {
  const t = useT();
  const pick = (color: string | null) => onEdit(setTrackColor(project, track.id, color));
  return (
    <fieldset className="color-field">
      <legend>{t("track.color")}</legend>
      <div className="swatches">
        {TRACK_COLORS.map(({ id, hex }) => {
          const name = t(`track.color.${id}` as MessageKey);
          return (
            <button
              key={id}
              type="button"
              className="swatch"
              style={{ background: hex }}
              aria-pressed={track.color === hex}
              aria-label={t("track.color.set", { color: name })}
              title={name}
              onClick={() => pick(hex)}
            />
          );
        })}
      </div>
      <button
        type="button"
        className="button"
        aria-pressed={track.color === undefined}
        disabled={track.color === undefined}
        onClick={() => pick(null)}
      >
        {t("track.color.default")}
      </button>
    </fieldset>
  );
}

function SelectionPanel({
  project,
  selection,
  audio,
  onEdit,
  onDelete,
}: {
  project: Project;
  selection: Selected | null;
  audio: Record<string, AssetAudio>;
  onEdit: (project: Project) => void;
  onDelete: () => void;
}) {
  const t = useT();
  const found = selection ? findItem(project, selection.id) : null;
  const { frameRate, start } = project.timecode;

  if (!found) return <p className="empty-note">{t("inspector.nothing")}</p>;

  if (found.kind === "clip") {
    const { clip, track } = found;
    const asset = project.assets.find((a) => a.id === clip.asset);
    return (
      <div className="panel-body">
        <h3>{t("clip.title")}</h3>
        <dl className="facts">
          <dt>{t("clip.name")}</dt>
          <dd>{asset?.name ?? clip.asset}</dd>
          <dt>{t("clip.track")}</dt>
          <dd>{track.name}</dd>
        </dl>
        <TimecodeField
          label={t("clip.start")}
          frame={start + clip.start}
          rate={frameRate}
          onCommit={(frame) => onEdit(updateClip(project, clip.id, { start: Math.max(0, frame - start) }))}
        />
        <TextField
          label={t("clip.gain")}
          value={String(clip.gainDb)}
          inputMode="decimal"
          monospace
          validate={(text) => (Number.isFinite(Number(text)) && text.trim() !== "" ? null : t("field.numberInvalid"))}
          onCommit={(text) => onEdit(updateClip(project, clip.id, { gainDb: Number(text) }))}
        />
        {!audio[clip.asset]?.peaks && <p className="field-note">{t("clip.lengthGuess")}</p>}
        <TrackColorField project={project} track={track} onEdit={onEdit} />
        <button type="button" className="button danger" onClick={onDelete}>
          <TrashIcon />
          {t("inspector.delete")}
        </button>
      </div>
    );
  }

  const { trigger } = found;
  const action = trigger.action;
  const setAction = (next: TriggerAction) => onEdit(updateTrigger(project, trigger.id, { action: next }));
  return (
    <div className="panel-body">
      <h3>{t("trigger.title")}</h3>
      <TextField
        label={t("trigger.label")}
        value={trigger.label}
        onCommit={(label) => onEdit(updateTrigger(project, trigger.id, { label }))}
      />
      <TimecodeField
        label={t("trigger.at")}
        frame={start + trigger.at}
        rate={frameRate}
        onCommit={(frame) => onEdit(updateTrigger(project, trigger.id, { at: Math.max(0, frame - start) }))}
      />
      <SelectField
        label={t("trigger.kind")}
        value={action.type}
        onChange={(kind) => setAction(kind === "midi" ? { type: "midi", bytes: [0xc0, 0x00] } : { type: "marker" })}
      >
        <option value="marker">{t("trigger.kind.marker")}</option>
        <option value="midi">{t("trigger.kind.midi")}</option>
      </SelectField>
      {action.type === "midi" && (
        <TextField
          label={t("trigger.bytes")}
          value={hex(action.bytes)}
          monospace
          validate={(text) => (parseBytes(text) ? null : t("trigger.bytesInvalid"))}
          onCommit={(text) => {
            const bytes = parseBytes(text);
            if (bytes) setAction({ type: "midi", bytes });
          }}
        />
      )}
      <TrackColorField project={project} track={found.track} onEdit={onEdit} />
      <button type="button" className="button danger" onClick={onDelete}>
        <TrashIcon />
        {t("inspector.delete")}
      </button>
    </div>
  );
}

export function Inspector() {
  const collapsed = useUi((ui) => ui.collapsed);
  const selection = useUi((ui) => ui.selection);
  const project = useDoc((doc) => doc.project);
  const audio = useDoc((doc) => doc.audio);
  const t = useT();
  const toggleLabel = `${t(collapsed ? "inspector.expand" : "inspector.collapse")} (${MOD_KEY}+B)`;
  const toggle = (
    <button
      type="button"
      className="icon-button inspector-toggle"
      aria-expanded={!collapsed}
      aria-controls="inspector-body"
      aria-label={t(collapsed ? "inspector.expand" : "inspector.collapse")}
      title={toggleLabel}
      onClick={toggleInspector}
    >
      {collapsed ? <ChevronLeftIcon /> : <ChevronRightIcon />}
    </button>
  );

  if (collapsed) {
    return (
      <aside className="inspector" id="inspector-body" data-collapsed="true" aria-label={t("inspector.legend")}>
        {toggle}
      </aside>
    );
  }

  return (
    <aside className="inspector" id="inspector-body" aria-label={t("inspector.legend")}>
      <div className="tab-row">
        {toggle}
        <h2 className="inspector-title">{t("inspector.tab.selection")}</h2>
      </div>
      <div className="panel">
        <SelectionPanel project={project} selection={selection} audio={audio} onEdit={edit} onDelete={deleteSelection} />
      </div>
    </aside>
  );
}
