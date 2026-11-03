import { LOCALES, useI18n } from "../i18n";
import type { Locale } from "../i18n";
import { WindowControls, hasNativeWindow } from "./WindowControls";

/** The modifier key as this platform prints it, for shortcut hints. */
export const MOD_KEY = /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";

export function MenuBar({
  name,
  dirty,
  canUndo,
  canRedo,
  onNew,
  onOpen,
  onSave,
  onSaveAs,
  onImport,
  onUndo,
  onRedo,
}: {
  name: string;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  onNew: () => void;
  onOpen: () => void;
  onSave: () => void;
  onSaveAs: () => void;
  onImport: () => void;
  onUndo: () => void;
  onRedo: () => void;
}) {
  const { t, locale, setLocale } = useI18n();
  const mod = MOD_KEY;
  // Shortcuts live in the tooltip: printed on every button they crowd a bar this narrow.
  const hint = (label: string, keys: string) => `${label} (${keys})`;
  return (
    <nav className="menubar" aria-label={t("app.title")} data-tauri-drag-region>
      <p className="brand" data-tauri-drag-region>{t("app.title")}</p>
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
        <button type="button" onClick={onNew} title={hint(t("menu.new"), `${mod}+N`)}>{t("menu.new")}</button>
        <button type="button" onClick={onOpen} title={hint(t("menu.open"), `${mod}+O`)}>{t("menu.open")}</button>
        <button type="button" onClick={onSave} title={hint(t("menu.save"), `${mod}+S`)}>{t("menu.save")}</button>
        <button type="button" onClick={onSaveAs} title={hint(t("menu.saveAs"), `${mod}+⇧+S`)}>{t("menu.saveAs")}</button>
        <button type="button" onClick={onImport} title={hint(t("menu.importAudio"), "I")}>{t("menu.importAudio")}</button>
      </div>
      <div className="menu-group" role="group" aria-label={t("menu.edit")}>
        <button type="button" onClick={onUndo} disabled={!canUndo} title={hint(t("menu.undo"), `${mod}+Z`)}>{t("menu.undo")}</button>
        <button type="button" onClick={onRedo} disabled={!canRedo} title={hint(t("menu.redo"), `${mod}+⇧+Z`)}>{t("menu.redo")}</button>
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
