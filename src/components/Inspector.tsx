import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { FrameRate, Project, TriggerAction } from "../project/types";
import { findItem, setFrameRate, setName, setStart, updateClip, updateTrigger } from "../state/edit";
import { SelectField, TextField, TimecodeField } from "./fields";
import { TrashIcon } from "./icons";
import { OutputPanel } from "./OutputPanel";
import type { Devices } from "../devices/useDevices";
import type { Playback } from "../transport/playback";
import type { Selected } from "./TrackRow";

export type InspectorTab = "selection" | "project" | "output";

const FRAME_RATES: FrameRate[] = ["fps24", "fps25", "fps2997Drop", "fps30"];
const TABS: InspectorTab[] = ["selection", "project", "output"];

const hex = (bytes: number[]) => bytes.map((b) => b.toString(16).toUpperCase().padStart(2, "0")).join(" ");

/** Parses "C0 05" into bytes, or `null` when it is not a list of hex bytes. */
function parseBytes(text: string): number[] | null {
  const parts = text.trim().split(/\s+/);
  if (parts.length === 0 || parts[0] === "") return null;
  const bytes = parts.map((part) => (/^[0-9a-f]{1,2}$/i.test(part) ? parseInt(part, 16) : NaN));
  return bytes.some(Number.isNaN) ? null : bytes;
}

function SelectionPanel({
  project,
  selection,
  onEdit,
  onDelete,
}: {
  project: Project;
  selection: Selected | null;
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
        <p className="field-note">{t("clip.lengthGuess")}</p>
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

export function Inspector({
  tab,
  onTab,
  project,
  selection,
  devices,
  playback,
  onEdit,
  onDelete,
}: {
  tab: InspectorTab;
  onTab: (tab: InspectorTab) => void;
  project: Project;
  selection: Selected | null;
  devices: Devices;
  playback: Playback;
  onEdit: (project: Project) => void;
  onDelete: () => void;
}) {
  const t = useT();
  return (
    <aside className="inspector" aria-label={t("inspector.legend")}>
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
            onClick={() => onTab(id)}
            onKeyDown={(event) => {
              const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
              if (step === 0) return;
              const next = TABS[(TABS.indexOf(id) + step + TABS.length) % TABS.length];
              onTab(next);
              document.getElementById(`tab-${next}`)?.focus();
            }}
          >
            {t(`inspector.tab.${id}` as MessageKey)}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`} className="panel">
        {tab === "selection" && (
          <SelectionPanel project={project} selection={selection} onEdit={onEdit} onDelete={onDelete} />
        )}
        {tab === "project" && <ProjectPanel project={project} onEdit={onEdit} />}
        {tab === "output" && <OutputPanel project={project} devices={devices} playback={playback} onEdit={onEdit} />}
      </div>
    </aside>
  );
}
