import { useState } from "react";
import { useT } from "../i18n";
import { useDoc, useTimeline } from "../state/store";
import { useShow } from "./store";
import { editShow } from "./api";
import type { SetlistEntry, TimelineControl, Transition } from "./types";

/** The project owns ordering and validation; fields only send editing intent. */
export function SetlistEditor() {
  const t = useT();
  const project = useDoc((d) => d.project);
  const timeline = useTimeline();
  const show = useShow((s) => s.state);
  const [selected, setSelected] = useState("");
  const [name, setName] = useState("");
  const [timelineId, setTimelineId] = useState(timeline.id);
  const [kind, setKind] = useState<"wait" | "loop">("wait");
  const [at, setAt] = useState(0);
  const [end, setEnd] = useState(25);
  const [turns, setTurns] = useState(0);
  const setlists = project.setlists ?? [];
  const setlist = setlists.find((s) => s.id === selected) ?? setlists[0];
  const locked = !!show && show.tick.status !== "ready" && show.tick.status !== "completed";
  const entryEdit = (entry: SetlistEntry) => setlist && void editShow({ op: "setEntry", setlistId: setlist.id, entry });
  const controlEdit = (control: TimelineControl) => void editShow({ op: "putControl", timelineId: timeline.id, control });
  return <details className="show-editor">
    <summary>{t("show.editor")}</summary>
    <fieldset disabled={locked} className="show-editor-body">
      <legend className="visually-hidden">{t("show.editor")}</legend>
      <div className="show-editor-heading">
        <label>{t("show.setlist")}<select value={setlist?.id ?? ""} onChange={(e) => setSelected(e.target.value)}>
          {!setlist && <option value="">{t("show.empty")}</option>}
          {setlists.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
        </select></label>
        <button className="button" type="button" onClick={() => void editShow({op: "addSetlist", name: t("show.default")})}>{t("show.addSetlist")}</button>
        {setlist && <>
          <input key={`${setlist.id}:${setlist.name}`} aria-label={t("show.name")} defaultValue={setlist.name} onBlur={(e) => {if (e.target.value !== setlist.name) void editShow({op: "renameSetlist", id: setlist.id, name: e.target.value});}} />
          <button className="button" type="button" onClick={() => void editShow({op: "removeSetlist", id: setlist.id})}>{t("show.removeSetlist")}</button>
        </>}
      </div>
      {setlist && <>
        <div className="show-table-scroll"><table className="show-table"><thead><tr>
          <th>{t("show.included")}</th><th>{t("show.timeline")}</th><th>{t("show.name")}</th><th>{t("show.notes")}</th><th>{t("show.transition")}</th><th>{t("show.order")}</th>
        </tr></thead><tbody>{setlist.entries.map((entry, index) => <tr key={entry.id} data-omitted={entry.omitted}>
          <td><input type="checkbox" aria-label={t("show.included")} checked={!entry.omitted} onChange={(e) => entryEdit({...entry, omitted: !e.target.checked})} /></td>
          <td>{project.timelines.find((tl) => tl.id === entry.timelineId)?.name}</td>
          <td><input aria-label={t("show.name")} key={entry.name} defaultValue={entry.name} onBlur={(e) => {if (e.target.value !== entry.name) entryEdit({...entry, name: e.target.value});}} /></td>
          <td><input aria-label={t("show.notes")} key={entry.notes} defaultValue={entry.notes} onBlur={(e) => {if (e.target.value !== entry.notes) entryEdit({...entry, notes: e.target.value});}} /></td>
          <td><select aria-label={t("show.transition")} value={entry.transition.type} onChange={(e) => entryEdit({...entry, transition: (e.target.value === "after" ? {type: "after", milliseconds: 1000} : {type: e.target.value}) as Transition})}>
            <option value="wait">{t("show.transition.wait")}</option><option value="continue">{t("show.transition.continue")}</option><option value="after">{t("show.transition.after")}</option>
          </select>{entry.transition.type === "after" && <input type="number" min={0} max={86400000} aria-label={t("show.delayMs")} defaultValue={entry.transition.milliseconds} key={entry.transition.milliseconds} onBlur={(e) => entryEdit({...entry, transition: {type:"after", milliseconds:e.target.valueAsNumber}})} />}</td>
          <td><div className="button-row">
            <button className="button" type="button" disabled={index === 0} aria-label={t("show.up")} onClick={() => void editShow({op:"moveEntry",setlistId:setlist.id,id:entry.id,to:index-1})}>↑</button>
            <button className="button" type="button" disabled={index === setlist.entries.length-1} aria-label={t("show.down")} onClick={() => void editShow({op:"moveEntry",setlistId:setlist.id,id:entry.id,to:index+1})}>↓</button>
            <button className="button" type="button" onClick={() => void editShow({op:"removeEntry",setlistId:setlist.id,id:entry.id})}>{t("show.remove")}</button>
          </div></td>
        </tr>)}</tbody></table></div>
        {setlist.entries.length === 0 && <p>{t("show.emptyEntries")}</p>}
        <div className="button-row"><select aria-label={t("show.timeline")} value={timelineId} onChange={(e) => setTimelineId(e.target.value)}>{project.timelines.map((tl) => <option key={tl.id} value={tl.id}>{tl.name}</option>)}</select>
          <button className="button" type="button" onClick={() => void editShow({op:"addEntry",setlistId:setlist.id,timelineId})}>{t("show.addEntry")}</button>
        </div>
      </>}
      <h3 className="legend">{t("show.controls", {name:timeline.name})}</h3>
      <p className="field-note">{t("show.controlNote")}</p>
      <div className="show-table-scroll"><table className="show-table"><thead><tr><th>{t("show.name")}</th><th>{t("show.kind")}</th><th>{t("show.at")}</th><th>{t("show.end")}</th><th>{t("show.turns")}</th><th /></tr></thead><tbody>
        {(timeline.controls ?? []).map((control) => <tr key={control.id}>
          <td><input aria-label={t("show.name")} key={control.name} defaultValue={control.name} onBlur={(e) => {if (e.target.value !== control.name) controlEdit({...control,name:e.target.value});}} /></td>
          <td>{t(control.type === "loop" ? "show.loop" : "show.wait")}</td>
          <td><input type="number" min={0} aria-label={t("show.at")} key={control.at} defaultValue={control.at} onBlur={(e) => {if (e.target.valueAsNumber !== control.at) controlEdit({...control,at:e.target.valueAsNumber});}} /></td>
          <td>{control.type === "loop" && <input type="number" min={1} aria-label={t("show.end")} key={control.end} defaultValue={control.end} onBlur={(e) => {if (e.target.valueAsNumber !== control.end) controlEdit({...control,end:e.target.valueAsNumber});}} />}</td>
          <td>{control.type === "loop" && <input type="number" min={0} aria-label={t("show.turns")} key={control.turns} defaultValue={control.turns ?? 0} onBlur={(e) => controlEdit({...control,turns:e.target.valueAsNumber === 0 ? null : e.target.valueAsNumber})} />}</td>
          <td><button className="button" type="button" onClick={() => void editShow({op:"removeControl",timelineId:timeline.id,id:control.id})}>{t("show.remove")}</button></td>
        </tr>)}
      </tbody></table></div>
      <form className="show-control-form" onSubmit={(e) => {e.preventDefault(); controlEdit(kind === "wait" ? {id:"",name:name || t("show.wait"),at,type:"wait"} : {id:"",name:name || t("show.loop"),at,type:"loop",end,turns: turns === 0 ? null : turns});}}>
        <label>{t("show.kind")}<select value={kind} onChange={(e) => setKind(e.target.value as "loop" | "wait")}><option value="wait">{t("show.wait")}</option><option value="loop">{t("show.loop")}</option></select></label>
        <label>{t("show.name")}<input value={name} onChange={(e) => setName(e.target.value)} /></label>
        <label>{t("show.at")}<input type="number" min={0} required value={at} onChange={(e) => setAt(e.target.valueAsNumber)} /></label>
        {kind === "loop" && <><label>{t("show.end")}<input type="number" min={1} required value={end} onChange={(e) => setEnd(e.target.valueAsNumber)} /></label><label>{t("show.turns")}<input type="number" min={0} required value={turns} onChange={(e) => setTurns(e.target.valueAsNumber)} /></label></>}
        <button className="button" type="submit">{t("show.addControl")}</button>
      </form>
    </fieldset>
    {locked && <p role="status">{t("show.locked")}</p>}
  </details>;
}
