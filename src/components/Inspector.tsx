import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { FadeCurve, Project, Timeline, Track } from "../project/types";
import { deleteSelection, edit, nameClip, nameTrack, setTimelineTempo } from "../state/actions";
import { editItem } from "../state/items";
import { detectTempo } from "../state/audio";
import type { AssetAudio } from "../state/doc";
import { useDoc, useTimeline } from "../state/store";
import { toggleInspector, useUi } from "../state/ui";
import type { Selected } from "../state/ui";
import { MAX_BPM, MIN_BPM } from "../project/types";
import {
  findItem,
  setAssetBpm,
  setNoteLength,
  setTrackColor,
  updateClip,
  updateNote,
} from "../state/edit";
import { TRACK_COLORS } from "../timeline/palette";
import { MidiEventPanel, OscEventPanel } from "./CueEventPanel";
import { FADE_CURVES } from "../audio/fade";
import { SelectField, TextAreaField, TextField, TimecodeField } from "./fields";
import { ChevronLeftIcon, ChevronRightIcon, TrashIcon } from "./icons";
import { MOD_KEY } from "./menus";

/** The colour of the track an item sits on. Each swatch is named, so colour is never the only cue. */
function TrackColorField({
  project,
  timeline,
  track,
  onEdit,
}: {
  project: Project;
  timeline: string;
  track: Track;
  onEdit: (project: Project) => void;
}) {
  const t = useT();
  const pick = (color: string | null) => onEdit(setTrackColor(project, timeline, track.id, color));
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

/** The name of the track an item is on, which the track header also types in place. */
function TrackNameField({ track }: { track: Track }) {
  const t = useT();
  return (
    <TextField
      // Another track starts the field over, so a half-typed name never lands on it.
      key={track.id}
      label={t("track.name")}
      value={track.name}
      onCommit={(text) => void nameTrack(track.id, text)}
    />
  );
}

function SelectionPanel({
  project,
  timeline,
  selection,
  audio,
  onEdit,
  onDelete,
}: {
  project: Project;
  timeline: Timeline;
  selection: Selected | null;
  audio: Record<string, AssetAudio>;
  onEdit: (project: Project) => void;
  onDelete: () => void;
}) {
  const t = useT();
  const found = selection ? findItem(project, selection.id) : null;
  const { frameRate, start } = timeline.timecode;

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
        </dl>
        <TextField
          key={clip.id}
          label={t("clip.label")}
          hint={t("clip.label.hint", { file: asset?.name ?? clip.asset })}
          value={clip.name ?? ""}
          onCommit={(text) => void nameClip(clip.id, text)}
        />
        <TrackNameField track={track} />
        <TimecodeField
          label={t("clip.start")}
          frame={start + clip.start}
          rate={frameRate}
          onCommit={(frame) => onEdit(updateClip(project, timeline.id, clip.id, { start: Math.max(0, frame - start) }))}
        />
        <TextField
          label={t("clip.gain")}
          value={String(clip.gainDb)}
          inputMode="decimal"
          monospace
          validate={(text) => (Number.isFinite(Number(text)) && text.trim() !== "" ? null : t("field.numberInvalid"))}
          onCommit={(text) => onEdit(updateClip(project, timeline.id, clip.id, { gainDb: Number(text) }))}
        />
        {[
          { label: t("clip.fadeInCurve"), key: "fadeInCurve", length: clip.fadeIn },
          { label: t("clip.fadeOutCurve"), key: "fadeOutCurve", length: clip.fadeOut },
        ].map(({ label, key, length }) => (
          <SelectField
            key={key}
            label={label}
            value={clip[key as "fadeInCurve" | "fadeOutCurve"] ?? "linear"}
            disabled={!length}
            hint={length ? undefined : t("clip.fadeCurve.none")}
            onChange={(curve) =>
              void editItem({
                op: "setFadeCurve",
                id: clip.id,
                fade: key === "fadeInCurve" ? "in" : "out",
                curve: curve as FadeCurve,
              })
            }
          >
            {FADE_CURVES.map((curve) => (
              <option key={curve} value={curve}>
                {t(`fade.curve.${curve}`)}
              </option>
            ))}
          </SelectField>
        ))}
        <p className="field-note">{t("envelope.hint")}</p>
        {clip.envelope && clip.envelope.length > 0 && (
          <button type="button" className="button" onClick={() => void editItem({ op: "clearEnvelope", id: clip.id })}>
            {t("envelope.remove")}
          </button>
        )}
        {!audio[clip.asset]?.peaks && <p className="field-note">{t("clip.lengthGuess")}</p>}
        {asset && (
          <>
            <TextField
              // Another file starts the field over, so a half-typed value never lands on it.
              key={asset.id}
              label={t("clip.bpm")}
              hint={t("clip.bpm.hint")}
              value={asset.bpm === undefined ? "" : String(asset.bpm)}
              inputMode="decimal"
              monospace
              validate={(text) => {
                if (text.trim() === "") return null;
                const bpm = Number(text.replace(",", "."));
                return Number.isFinite(bpm) && bpm >= MIN_BPM && bpm <= MAX_BPM
                  ? null
                  : t("tempo.bpm.invalid", { min: MIN_BPM, max: MAX_BPM });
              }}
              onCommit={(text) =>
                onEdit(setAssetBpm(project, asset.id, text.trim() === "" ? null : Number(text.replace(",", "."))))
              }
            />
            <div className="button-row">
              <button type="button" className="button" onClick={() => void detectTempo(asset, true)}>
                {t("clip.bpm.detect")}
              </button>
              <button
                type="button"
                className="button"
                disabled={asset.bpm === undefined}
                onClick={() => asset.bpm !== undefined && setTimelineTempo({ bpm: asset.bpm })}
              >
                {t("clip.bpm.use")}
              </button>
            </div>
          </>
        )}
        <TrackColorField project={project} timeline={timeline.id} track={track} onEdit={onEdit} />
        <button type="button" className="button danger" onClick={onDelete}>
          <TrashIcon />
          {t("inspector.delete")}
        </button>
      </div>
    );
  }

  if (found.kind === "midiEvent") {
    return <MidiEventPanel track={found.track} event={found.event} start={start} rate={frameRate} />;
  }
  if (found.kind === "oscEvent") {
    return (
      <OscEventPanel
        track={found.track}
        event={found.event}
        start={start}
        rate={frameRate}
        destinations={project.output.osc ?? []}
      />
    );
  }

  const { note } = found;
  return (
    <div className="panel-body">
      <h3>{t("note.title")}</h3>
      <TrackNameField track={found.track} />
      <TextAreaField
        // A new note is selected to be written: it opens with the cursor in the text.
        key={note.id}
        label={t("note.text")}
        value={note.text}
        autoFocus={note.text === ""}
        onCommit={(text) => onEdit(updateNote(project, timeline.id, note.id, { text }))}
      />
      <TimecodeField
        label={t("note.at")}
        frame={start + note.at}
        rate={frameRate}
        onCommit={(frame) => onEdit(updateNote(project, timeline.id, note.id, { at: Math.max(0, frame - start) }))}
      />
      <TimecodeField
        label={t("note.length")}
        hint={t("note.length.hint")}
        frame={note.length ?? 0}
        rate={frameRate}
        onCommit={(frames) => onEdit(setNoteLength(project, timeline.id, note.id, frames))}
      />
      <TrackColorField project={project} timeline={timeline.id} track={found.track} onEdit={onEdit} />
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
  const timeline = useTimeline();
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
        <SelectionPanel project={project} timeline={timeline} selection={selection} audio={audio} onEdit={edit} onDelete={deleteSelection} />
      </div>
    </aside>
  );
}
