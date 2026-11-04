import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { FrameRate, Project, Track, TriggerAction } from "../project/types";
import { deleteSelection, edit } from "../state/actions";
import type { AssetAudio } from "../state/doc";
import { useDoc } from "../state/store";
import { openTab, toggleInspector, useUi } from "../state/ui";
import type { InspectorTab, Selected } from "../state/ui";
import { findItem, setFrameRate, setName, setStart, setTrackColor, updateClip, updateTrigger } from "../state/edit";
import { TRACK_COLORS } from "../timeline/palette";
import { SelectField, TextField, TimecodeField } from "./fields";
import { ChevronLeftIcon, ChevronRightIcon, OutputIcon, ProjectIcon, SelectionIcon, TrashIcon } from "./icons";
import { MOD_KEY } from "./MenuBar";
import { OutputPanel } from "./OutputPanel";
import type { Devices } from "../devices/useDevices";

const FRAME_RATES: FrameRate[] = ["fps24", "fps25", "fps2997Drop", "fps30"];
const TABS: InspectorTab[] = ["selection", "project", "output"];
const TAB_ICONS = { selection: SelectionIcon, project: ProjectIcon, output: OutputIcon };

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

function ProjectPanel({ project, onEdit }: { project: Project; onEdit: (project: Project) => void }) {
  const t = useT();
  const { frameRate, start } = project.timecode;
  return (
    <div className="panel-body">
      <TextField label={t("project.name")} value={project.name} onCommit={(name) => onEdit(setName(project, name))} />
      <SelectField
        label={t("project.frameRate")}
        value={frameRate}
        hint={t("project.frameRateNote")}
        onChange={(rate) => onEdit(setFrameRate(project, rate as FrameRate))}
      >
        {FRAME_RATES.map((rate) => (
          <option key={rate} value={rate}>
            {t(`frameRate.${rate}` as MessageKey)}
          </option>
        ))}
      </SelectField>
      <TimecodeField
        label={t("project.start")}
        frame={start}
        rate={frameRate}
        onCommit={(frame) => onEdit(setStart(project, frame))}
      />
    </div>
  );
}

export function Inspector({ devices }: { devices: Devices }) {
  const tab = useUi((ui) => ui.tab);
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
        <div className="rail">
          {TABS.map((id) => {
            const TabIcon = TAB_ICONS[id];
            const label = t(`inspector.tab.${id}` as MessageKey);
            return (
              <button key={id} type="button" className="icon-button" aria-label={label} title={label} onClick={() => openTab(id)}>
                <TabIcon />
              </button>
            );
          })}
        </div>
      </aside>
    );
  }

  return (
    <aside className="inspector" id="inspector-body" aria-label={t("inspector.legend")}>
      <div className="tab-row">
        {toggle}
        <div className="tabs" role="tablist">
          {TABS.map((id) => (
            <button
              key={id}
              type="button"
              role="tab"
              id={`tab-${id}`}
              aria-selected={tab === id}
              aria-controls={`panel-${id}`}
              tabIndex={tab === id ? 0 : -1}
              onClick={() => openTab(id)}
              onKeyDown={(event) => {
                const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
                if (step === 0) return;
                const next = TABS[(TABS.indexOf(id) + step + TABS.length) % TABS.length];
                openTab(next);
                document.getElementById(`tab-${next}`)?.focus();
              }}
            >
              {t(`inspector.tab.${id}` as MessageKey)}
            </button>
          ))}
        </div>
      </div>
      <div role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`} className="panel">
        {tab === "selection" && (
          <SelectionPanel project={project} selection={selection} audio={audio} onEdit={edit} onDelete={deleteSelection} />
        )}
        {tab === "project" && <ProjectPanel project={project} onEdit={edit} />}
        {tab === "output" && <OutputPanel project={project} devices={devices} onEdit={edit} />}
      </div>
    </aside>
  );
}
