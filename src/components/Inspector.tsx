import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { Project, Track } from "../project/types";
import { deleteSelection, edit } from "../state/actions";
import type { AssetAudio } from "../state/doc";
import { useDoc } from "../state/store";
import { toggleInspector, useUi } from "../state/ui";
import type { Selected } from "../state/ui";
import {
  findItem,
  setNoteLength,
  setTrackColor,
  updateClip,
  updateNote,
} from "../state/edit";
import { TRACK_COLORS } from "../timeline/palette";
import { TextAreaField, TextField, TimecodeField } from "./fields";
import { ChevronLeftIcon, ChevronRightIcon, TrashIcon } from "./icons";
import { MOD_KEY } from "./MenuBar";

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

  const { note } = found;
  return (
    <div className="panel-body">
      <h3>{t("note.title")}</h3>
      <TextAreaField
        // A new note is selected to be written: it opens with the cursor in the text.
        key={note.id}
        label={t("note.text")}
        value={note.text}
        autoFocus={note.text === ""}
        onCommit={(text) => onEdit(updateNote(project, note.id, { text }))}
      />
      <TimecodeField
        label={t("note.at")}
        frame={start + note.at}
        rate={frameRate}
        onCommit={(frame) => onEdit(updateNote(project, note.id, { at: Math.max(0, frame - start) }))}
      />
      <TimecodeField
        label={t("note.length")}
        hint={t("note.length.hint")}
        frame={note.length ?? 0}
        rate={frameRate}
        onCommit={(frames) => onEdit(setNoteLength(project, note.id, frames))}
      />
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
