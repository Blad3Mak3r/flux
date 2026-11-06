# Welcome window flow design

## Scope

- Add **File > Close** in the editor. It follows the existing unsaved-changes flow; after saving or discarding, it opens the welcome window and closes the editor. Cancel keeps the editor open.
- Keep the launcher and welcome windows above other application windows. The editor remains a normal window.
- Align the welcome window controls with the editor controls at the far right of the title bar.
- Add a deterministic project badge to each welcome recent-project row: a square gradient with up to three initials from the first three whitespace-delimited words in the project name.

## Direction

The close action reuses the existing transition and unsaved-dialog ownership rather than introducing a second close path. The backend builds the welcome window before destroying the editor, so Flux never has zero windows.

Recent-project badges are presentation only. They derive from the displayed project name, do not alter `.fxtc` data, and use the established dark bench-instrument palette with square edges and high-contrast text. Examples: `Titanium` becomes `T`; `MegaSuper Proyecto To Guapo` becomes `MSP`.

## Boundaries and validation

- No changes to project data, recents persistence, or editor window priority.
- Native project file dialogs must still appear in front of the always-on-top welcome window; this needs manual validation on Windows, macOS, and Linux.
- Add focused tests for initials derivation and the menu/close transition where the existing test structure supports them.
- Run `npm run typecheck` and `npm test`; verify the native flows manually where available.
