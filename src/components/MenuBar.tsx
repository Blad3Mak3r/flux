import { LOCALES, useI18n } from "../i18n";
import { BrandMark } from "./BrandMark";
import type { Locale } from "../i18n";
import { isDirty } from "../state/doc";
import { dispatch, useDoc } from "../state/store";
import { useUi } from "../state/ui";
import { addNoteAt, importAudioFiles, newDocument, openDocument, saveDocument } from "../state/workflows";
import { WindowControls, hasNativeWindow } from "./WindowControls";

/** The modifier key as this platform prints it, for shortcut hints. */
export const MOD_KEY = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";

export function MenuBar() {
  const name = useDoc((doc) => doc.project.name);
  const dirty = useDoc(isDirty);
  const canUndo = useDoc((doc) => doc.past.length > 0);
  const canRedo = useDoc((doc) => doc.future.length > 0);
  const editing = useUi((ui) => ui.view === "edit");
  const { t, locale, setLocale } = useI18n();
  const mod = MOD_KEY;
  // Shortcuts live in the tooltip: printed on every button they crowd a bar this narrow.
  const hint = (label: string, keys: string) => `${label} (${keys})`;
  return (
    <nav className="menubar" aria-label={t("app.title")} data-tauri-drag-region>
      <BrandMark />
      <p className="doc-name" title={name} data-tauri-drag-region>
        {name}
        {dirty && (
          <span className="dirty" title={t("project.unsaved")}>
            <span aria-hidden="true">●</span>
            <span className="visually-hidden">{t("project.unsaved")}</span>
          </span>
        )}
      </p>

      <div className="menu-group" role="group" aria-label={t("menu.file")}>
        <button type="button" onClick={() => newDocument(t)} title={hint(t("menu.new"), `${mod}+N`)}>{t("menu.new")}</button>
        <button type="button" onClick={openDocument} title={hint(t("menu.open"), `${mod}+O`)}>{t("menu.open")}</button>
        <button type="button" onClick={() => void saveDocument(false)} title={hint(t("menu.save"), `${mod}+S`)}>{t("menu.save")}</button>
        <button type="button" onClick={() => void saveDocument(true)} title={hint(t("menu.saveAs"), `${mod}+⇧+S`)}>{t("menu.saveAs")}</button>
        <button type="button" onClick={() => void importAudioFiles()} title={hint(t("menu.importAudio"), "I")}>{t("menu.importAudio")}</button>
        <button type="button" onClick={() => addNoteAt(t)} disabled={!editing} title={hint(t("menu.addNote"), "N")}>{t("menu.addNote")}</button>
      </div>
      <div className="menu-group" role="group" aria-label={t("menu.edit")}>
        <button type="button" onClick={() => dispatch({ type: "undo" })} disabled={!canUndo} title={hint(t("menu.undo"), `${mod}+Z`)}>{t("menu.undo")}</button>
        <button type="button" onClick={() => dispatch({ type: "redo" })} disabled={!canRedo} title={hint(t("menu.redo"), `${mod}+⇧+Z`)}>{t("menu.redo")}</button>
      </div>

      <div className="menu-end">
        <label className="locale">
          <span className="visually-hidden">{t("menu.language")}</span>
          <select value={locale} onChange={(event) => setLocale(event.target.value as Locale)}>
            {LOCALES.map((option) => (
              <option key={option.id} value={option.id}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      {hasNativeWindow() && <WindowControls />}
    </nav>
  );
}
