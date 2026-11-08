import { useState } from "react";
import { useT } from "../i18n";
import { ArrowDownIcon, ArrowUpIcon, ChevronLeftIcon, ListIcon, PlusIcon, TrashIcon } from "../components/icons";
import { TextAreaField, TextField, SelectField } from "../components/fields";
import { ViewSwitch } from "../live/ViewSwitch";
import { useDoc } from "../state/store";
import { openTimelineEditor, setView, useUi } from "../state/ui";
import { selectTimeline } from "../state/timelines";
import { useShow } from "./store";
import { editShow } from "./api";
import type { Setlist, SetlistEntry, Transition } from "./types";

export function SetlistEditor() {
  const t = useT();
  const project = useDoc((d) => d.project);
  const running = useShow((s) => s.state);
  const selected = useUi((s) => s.setlistSelection);
  const entryId = useUi((s) => s.setlistEntrySelection);
  const setSelected = (setlistSelection: string) => useUi.setState({setlistSelection});
  const setEntryId = (setlistEntrySelection: string) => useUi.setState({setlistEntrySelection});
  const [adding, setAdding] = useState(false);
  const [settings, setSettings] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [timelineId, setTimelineId] = useState(project.timelines[0]?.id ?? "");
  const lists = project.setlists ?? [];
  const setlist = lists.find((s) => s.id === selected) ?? lists[0];
  const entry = setlist?.entries.find((e) => e.id === entryId) ?? setlist?.entries[0];
  const locked = !!running && running.tick.status !== "ready" && running.tick.status !== "completed";
  const create = async () => {
    const result = await editShow({op:"addSetlist",name:newName || t("show.default")});
    const updated = result?.project.setlists;
    const created = updated?.[updated.length-1];
    if (created) {setSelected(created.id); setEntryId(""); setCreating(false); setNewName(""); setSettings(false);}
  };
  const add = async () => {
    if (!setlist) return;
    const result = await editShow({op:"addEntry",setlistId:setlist.id,timelineId});
    const entries = result?.project.setlists?.find((s) => s.id === setlist.id)?.entries;
    const created = entries?.[entries.length-1];
    if (created) {setEntryId(created.id); setAdding(false); setSettings(false);}
  };
  return <section className="setlist-workspace screen" aria-labelledby="setlist-title">
    <header className="screen-bar">
      <button type="button" className="button" onClick={openTimelineEditor}><ChevronLeftIcon />{t("show.backTimeline")}</button>
      <h2 id="setlist-title" className="legend">{t("show.workspace")}</h2>
      <ViewSwitch />
    </header>
    <div className="setlist-heading">
      <div><h3>{setlist?.name ?? t("show.workspace")}</h3><p>{t("show.workspaceNote")}</p></div>
      <div className="button-row">
        {setlist && <select aria-label={t("show.setlist")} value={setlist.id} disabled={locked} onChange={(e) => {setSelected(e.target.value); setEntryId(""); setSettings(false);}}>{lists.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}</select>}
        {setlist && <button type="button" className="button" disabled={locked} onClick={() => setCreating(!creating)}><PlusIcon />{t("show.addSetlist")}</button>}
        {setlist && <button type="button" className="button" aria-pressed={settings} onClick={() => setSettings(!settings)}>{t("show.setlistSettings")}</button>}
      </div>
    </div>
    {creating && <form className="setlist-create" onSubmit={(e) => {e.preventDefault(); void create();}}>
      <label htmlFor="new-setlist-name">{t("show.setlistName")}</label>
      <input id="new-setlist-name" autoFocus value={newName} placeholder={t("show.default")} onChange={(e) => setNewName(e.target.value)} />
      <button type="submit" className="button primary">{t("show.create")}</button>
      <button type="button" className="button" onClick={() => setCreating(false)}>{t("dialog.cancel")}</button>
    </form>}
    {locked && <p className="setlist-lock" role="status">{t("show.locked")}</p>}
    {!setlist ? <div className="setlist-empty"><ListIcon /><h3>{t("show.emptyTitle")}</h3><p>{t("show.emptyBody")}</p>
      <button type="button" className="button primary" onClick={() => setCreating(true)}>{t("show.addSetlist")}</button>
    </div> : <div className="setlist-columns">
      <div className="setlist-order">
        <div className="setlist-list-head"><h3>{t("show.runningOrder")}</h3><button type="button" className="button" disabled={locked} onClick={() => {setAdding(!adding); setSettings(false);}}><PlusIcon />{t("show.addEntry")}</button></div>
        <ol className="setlist-entries" aria-label={t("show.runningOrder")}>
          {setlist.entries.map((item,index) => {
            const timeline = project.timelines.find((tl) => tl.id === item.timelineId);
            return <li key={item.id} data-omitted={item.omitted}>
              <button type="button" className="setlist-entry" aria-pressed={!settings && entry?.id === item.id} onClick={() => {setEntryId(item.id); setSettings(false);}}>
                <span className="setlist-number">{index+1}</span>
                <span className="setlist-entry-copy"><strong>{item.name || timeline?.name}</strong><span>{t(`show.summary.${item.transition.type}`)}{item.omitted && <> · {t("show.omitted")}</>}</span></span>
                {item.notes && <span className="setlist-notes-indicator" title={t("show.hasNotes")}>{t("show.notesShort")}</span>}
              </button>
            </li>;
          })}
        </ol>
        {setlist.entries.length === 0 && <div className="setlist-empty-list"><h4>{t("show.emptyOrder")}</h4><p>{t("show.emptyEntries")}</p></div>}
        {adding && <form className="setlist-add" onSubmit={(e) => {e.preventDefault(); void add();}}>
          <label htmlFor="add-setlist-timeline">{t("show.chooseTimeline")}</label>
          <select id="add-setlist-timeline" autoFocus value={timelineId} onChange={(e) => setTimelineId(e.target.value)}>{project.timelines.map((tl) => <option key={tl.id} value={tl.id}>{tl.name}</option>)}</select>
          <div className="button-row"><button type="submit" className="button primary">{t("show.addEntry")}</button><button type="button" className="button" onClick={() => setAdding(false)}>{t("dialog.cancel")}</button></div>
        </form>}
        <footer className="setlist-order-foot"><p>{t("show.portableNote")}</p><button type="button" className="button" disabled={locked || setlist.entries.length === 0} onClick={() => setView("live")}>{t("show.openLive")}</button></footer>
      </div>
      <aside className="setlist-detail" aria-label={t("show.entrySettings")}>
        {settings ? <SetlistSettings key={setlist.id} setlist={setlist} locked={locked} /> : entry ? <EntrySettings key={entry.id} setlist={setlist} entry={entry} locked={locked} /> : <div className="setlist-detail-empty"><h3>{t("show.selectEntry")}</h3><p>{t("show.selectEntryNote")}</p></div>}
      </aside>
    </div>}
  </section>;
}

function SetlistSettings({setlist,locked}:{setlist:Setlist;locked:boolean}) {
  const t = useT();
  return <fieldset disabled={locked} className="setlist-property-fields"><legend>{t("show.setlistSettings")}</legend>
    <TextField label={t("show.setlistName")} value={setlist.name} onCommit={(name) => void editShow({op:"renameSetlist",id:setlist.id,name})} />
    <div className="setlist-danger"><p>{t("show.removeSetlistNote")}</p><button type="button" className="button danger" onClick={() => void editShow({op:"removeSetlist",id:setlist.id})}><TrashIcon />{t("show.removeSetlist")}</button></div>
  </fieldset>;
}

function EntrySettings({setlist,entry,locked}:{setlist:Setlist;entry:SetlistEntry;locked:boolean}) {
  const t = useT();
  const timeline = useDoc((d) => d.project.timelines.find((tl) => tl.id === entry.timelineId));
  const change = (patch:Partial<SetlistEntry>) => void editShow({op:"setEntry",setlistId:setlist.id,entry:{...entry,...patch}});
  const index = setlist.entries.indexOf(entry);
  return <fieldset disabled={locked} className="setlist-property-fields"><legend>{t("show.entrySettings")}</legend>
    <div className="setlist-source"><span>{t("show.timeline")}</span><strong>{timeline?.name}</strong><button type="button" className="button" onClick={() => {void (async () => {await selectTimeline(entry.timelineId); openTimelineEditor();})();}}>{t("show.editTimeline")}</button></div>
    <TextField label={t("show.entryName")} hint={t("show.entryNameHint")} value={entry.name} onCommit={(name) => change({name})} />
    <SelectField label={t("show.transition")} value={entry.transition.type} onChange={(type) => change({transition:type === "after" ? {type,milliseconds:1000} : {type} as Transition})}>
      <option value="wait">{t("show.transition.wait")}</option><option value="continue">{t("show.transition.continue")}</option><option value="after">{t("show.transition.after")}</option>
    </SelectField>
    <p className="field-note">{t(`show.explain.${entry.transition.type}`)}</p>
    {entry.transition.type === "after" && <label className="field">{t("show.delayMs")}<input type="number" min={0} defaultValue={entry.transition.milliseconds} key={entry.transition.milliseconds} onBlur={(e) => {if (entry.transition.type === "after" && e.target.valueAsNumber !== entry.transition.milliseconds) change({transition:{type:"after",milliseconds:e.target.valueAsNumber}});}} /></label>}
    <TextAreaField label={t("show.notes")} hint={t("show.notesHint")} value={entry.notes} rows={4} onCommit={(notes) => change({notes})} />
    <label className="check-row"><input type="checkbox" checked={!entry.omitted} onChange={(e) => change({omitted:!e.target.checked})} /><span>{t("show.includeEntry")}</span></label>
    <div className="setlist-entry-tools"><span>{t("show.order")}</span><div className="button-row">
      <button type="button" className="button" disabled={index === 0} onClick={() => void editShow({op:"moveEntry",setlistId:setlist.id,id:entry.id,to:index-1})}><ArrowUpIcon />{t("show.up")}</button>
      <button type="button" className="button" disabled={index === setlist.entries.length-1} onClick={() => void editShow({op:"moveEntry",setlistId:setlist.id,id:entry.id,to:index+1})}><ArrowDownIcon />{t("show.down")}</button>
    </div></div>
    <div className="setlist-danger"><button type="button" className="button danger" onClick={() => void editShow({op:"removeEntry",setlistId:setlist.id,id:entry.id})}><TrashIcon />{t("show.removeEntry")}</button></div>
  </fieldset>;
}
