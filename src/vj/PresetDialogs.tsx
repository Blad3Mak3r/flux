// The dialogs of presets and templates: the question for a name, and the lists to rename, delete and start from. The
// backend keeps the lists and checks the names; this asks and shows.

import { useEffect, useRef, useState } from "react";
import { useT } from "../i18n";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { CloseIcon, DuplicateIcon, PlusIcon } from "../components/icons";
import { newFromTemplate } from "../state/workflows";
import type { NameAsk } from "./presets";
import { closeAsk, deletePreset, deleteTemplate, managePresets, presetKind, renamePreset, renameTemplate, saveTemplate, usePresets } from "./presets";
import { effectKey } from "./effects";

/** Opens `dialog` while `open`, closes it when not. */
function useModal(open: boolean) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    if (!open && dialog.open) dialog.close();
  }, [open]);
  return ref;
}

/** The form of one question: its text starts as the question's value. */
function NameForm({ ask }: { ask: NameAsk }) {
  const t = useT();
  const [value, setValue] = useState(ask.value);
  const go = () => {
    if (value.trim() === "") return;
    closeAsk();
    void ask.run(value);
  };
  return (
    <form method="dialog" onSubmit={(event) => { event.preventDefault(); go(); }}>
      <h2 id="vj-name-title">{ask.title}</h2>
      <div className="field">
        <label htmlFor="vj-name-input">{t("vj.preset.name")}</label>
        <input id="vj-name-input" type="text" value={value} maxLength={64} autoFocus
          onFocus={(event) => event.currentTarget.select()} onChange={(event) => setValue(event.target.value)} />
      </div>
      <div className="button-row vj-name-actions">
        <button type="button" className="button" onClick={closeAsk}>{t("dialog.cancel")}</button>
        <button type="submit" className="button primary" disabled={value.trim() === ""}>{ask.confirm}</button>
      </div>
    </form>
  );
}

/** The question for a name: what is asked is the store's `ask`, and its answer is kept by the backend. */
export function NameDialog() {
  const ask = usePresets((state) => state.ask);
  const ref = useModal(ask !== null);
  return (
    <dialog ref={ref} className="dialog vj-name" onCancel={closeAsk} aria-labelledby="vj-name-title">
      {ask && <NameForm key={ask.title + ask.value} ask={ask} />}
    </dialog>
  );
}

/** The user's presets or templates, to rename, delete and, for templates, start a show from. */
export function LibraryDialog() {
  const t = useT();
  const manage = usePresets((state) => state.manage);
  const presets = usePresets((state) => state.presets);
  const templates = usePresets((state) => state.templates);
  const ref = useModal(manage !== null);
  const close = () => managePresets(null);
  // A delete cannot be undone, so it asks first.
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null);
  const remove = () => {
    if (!removing) return;
    void (manage === "presets" ? deletePreset(removing.id) : deleteTemplate(removing.id));
    setRemoving(null);
  };
  return (
    <dialog ref={ref} className="dialog vj-lib" onCancel={close} aria-labelledby="vj-lib-title">
      {manage && (
        <>
          <h2 id="vj-lib-title">{t(manage === "presets" ? "vj.preset.manage" : "vj.template.manage")}</h2>
          <p className="vj-dim">{t(manage === "presets" ? "vj.preset.where" : "vj.template.where")}</p>
          <ul className="vj-lib-list">
            {manage === "presets" && presets.map((preset) => {
              const kind = presetKind(preset);
              return (
                <li key={preset.id}>
                  <strong>{preset.name}</strong>
                  <span className="vj-dim">{kind ? t(effectKey(kind)) : t("vj.preset.stackOf", { n: preset.effects.length })}</span>
                  <button type="button" className="vj-tb" onClick={() => renamePreset(t, preset)}>{t("vj.preset.rename")}</button>
                  <button type="button" className="vj-fx-b" title={t("vj.preset.delete")} aria-label={`${t("vj.preset.delete")}: ${preset.name}`}
                    onClick={() => setRemoving({ id: preset.id, name: preset.name })}><CloseIcon /></button>
                </li>
              );
            })}
            {manage === "templates" && templates.map((template) => (
              <li key={template.id}>
                <strong>{template.name}</strong>
                <span className="vj-dim">{t("vj.template.summary", { layers: template.layers, columns: template.columns })}</span>
                <button type="button" className="vj-tb" title={t("vj.template.newNote")}
                  onClick={() => { close(); void newFromTemplate(t, template.id); }}><DuplicateIcon />{t("vj.template.new")}</button>
                <button type="button" className="vj-tb" onClick={() => renameTemplate(t, template)}>{t("vj.preset.rename")}</button>
                <button type="button" className="vj-fx-b" title={t("vj.preset.delete")} aria-label={`${t("vj.preset.delete")}: ${template.name}`}
                  onClick={() => setRemoving({ id: template.id, name: template.name })}><CloseIcon /></button>
              </li>
            ))}
          </ul>
          {(manage === "presets" ? presets : templates).length === 0 && (
            <p className="vj-dim vj-lib-empty">{t(manage === "presets" ? "vj.preset.none" : "vj.template.none")}</p>
          )}
          <div className="button-row">
            {manage === "templates" && (
              <button type="button" className="button" onClick={() => saveTemplate(t)}><PlusIcon />{t("vj.template.saveAs")}</button>
            )}
            <button type="button" className="button" onClick={close} autoFocus>{t("dialog.close")}</button>
          </div>
        </>
      )}
      <ConfirmDialog open={removing !== null} title={t(manage === "presets" ? "vj.preset.delete" : "vj.template.delete")} body={t("vj.library.deleteBody", { name: removing?.name ?? "" })}
        confirmLabel={t(manage === "presets" ? "vj.preset.delete" : "vj.template.delete")} onConfirm={remove} onCancel={() => setRemoving(null)} />
    </dialog>
  );
}
