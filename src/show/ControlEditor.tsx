import { useT } from "../i18n";
import { CloseIcon, LoopIcon, PauseIcon, PlusIcon, TrashIcon } from "../components/icons";
import { SelectField, TextField, TimecodeField } from "../components/fields";
import { useTimeline } from "../state/store";
import { closeShowControls, openShowControls, useUi } from "../state/ui";
import { useShow } from "./store";
import { addShowControl, editShow } from "./api";
import type { TimelineControl } from "./types";
import { formatFrames } from "../timecode";

/** One selected control, using the same inspector and time fields as clips and notes. */
export function ControlEditor() {
  const t = useT();
  const timeline = useTimeline();
  const selected = useUi((ui) => ui.controlEditor);
  const run = useShow((s) => s.state);
  const control = timeline.controls?.find((c) => c.id === selected);
  const locked = !!run && run.tick.status !== "ready" && run.tick.status !== "completed";
  const change = (next: TimelineControl) => void editShow({op:"putControl",timelineId:timeline.id,control:next});
  return <aside className="inspector control-inspector" aria-labelledby="control-title">
    <div className="tab-row"><h2 className="inspector-title" id="control-title">{t("show.loopsWaits")}</h2><button type="button" className="icon-button" aria-label={t("menu.close")} onClick={closeShowControls}><CloseIcon /></button></div>
    <div className="control-inspector-body">
      <p className="control-timeline-name">{timeline.name}</p>
      <fieldset disabled={locked} className="control-add-actions">
        <button type="button" className="button" onClick={() => void addShowControl("wait",t("show.wait"))}><PlusIcon /><PauseIcon />{t("show.addWait")}</button>
        <button type="button" className="button" onClick={() => void addShowControl("loop",t("show.loop"))}><PlusIcon /><LoopIcon />{t("show.addLoop")}</button>
      </fieldset>
      <p className="field-note control-context">{t("show.controlContext")}</p>
      {(timeline.controls?.length ?? 0) === 0 ? <p className="field-note">{t("show.controlsEmpty")}</p> : <ul className="control-list" aria-label={t("show.loopsWaits")}>
        {timeline.controls?.map((c) => <li key={c.id}><button type="button" aria-pressed={c.id === control?.id} onClick={() => openShowControls(c.id)}>{c.type === "loop" ? <LoopIcon /> : <PauseIcon />}<span className="control-list-copy"><strong>{c.name}</strong><span className="control-time-range">{formatFrames(c.at, timeline.timecode.frameRate)}{c.type === "loop" && <> – {formatFrames(c.end, timeline.timecode.frameRate)}</>}</span><span>{c.type === "wait" ? t("show.wait") : c.turns === null ? t("show.untilGo") : t("show.repeatCount", { count: c.turns })}</span></span></button></li>)}
      </ul>}
      {control ? <fieldset key={control.id} disabled={locked} className="control-properties">
        <legend>{t(control.type === "loop" ? "show.loop" : "show.wait")}</legend>
        <TextField label={t("show.name")} value={control.name} onCommit={(name) => change({...control,name})} />
        <TimecodeField label={t("show.position")} hint={t("show.relativeTime")} frame={control.at} rate={timeline.timecode.frameRate} onCommit={(at) => change({...control,at})} />
        {control.type === "loop" && <>
          <TimecodeField label={t("show.loopEnd")} frame={control.end} rate={timeline.timecode.frameRate} onCommit={(end) => change({...control,end})} />
          <SelectField label={t("show.repeat")} value={control.turns === null ? "go" : "count"} onChange={(value) => change({...control,turns:value === "go" ? null : 2})}>
            <option value="go">{t("show.untilGo")}</option><option value="count">{t("show.fixedTurns")}</option>
          </SelectField>
          {control.turns !== null && <label className="field">{t("show.totalTurns")}<input type="number" min={1} key={control.turns} defaultValue={control.turns} onBlur={(e) => {if (e.target.valueAsNumber !== control.turns) change({...control,turns:e.target.valueAsNumber});}} /></label>}
          <p className="field-note">{t("show.loopExplain")}</p>
        </>}
        {control.type === "wait" && <p className="field-note">{t("show.waitExplain")}</p>}
        <div className="setlist-danger"><button type="button" className="button danger" onClick={() => void editShow({op:"removeControl",timelineId:timeline.id,id:control.id}).then(() => openShowControls())}><TrashIcon />{t("show.removeControl")}</button></div>
      </fieldset> : <p className="field-note">{t("show.selectControl")}</p>}
      {locked && <p role="status">{t("show.locked")}</p>}
    </div>
  </aside>;
}
