# Show workspace refinements

Approved scope: the four priorities from the independent show-workspace review, in both Edit and Live. Preserve FLUX's Bench Instrument identity and existing sample-clock execution.

1. Prepared Live replaces project timeline tabs with its running order. Show entry numbers, aliases, source timelines, current/next/done state and omissions. Entry preparation remains available only while stopped; omitted entries cannot be prepared. Unprepared Live retains ordinary timeline navigation.
2. Backend-derived entry summaries expose exact transition delays, manual waits, indefinite loops and counted loops. Summaries update after edits, undo and redo. No project format migration is required.
3. Selected loops have an unmistakable start/end span. The control list shows each time range and repeat mode. Persistent guidance explains creation at the playhead and execution in prepared setlists.
4. A sticky running-order toolbar places Move up, Move down and Move to beside the selected entry. Alt+Up/Down provides keyboard movement from a focused running-order row. Backend edits retain existing validation and undo behavior.

Implementation: derive immutable overview data in flux-project, retain the prepared overview with ShowPlan, and publish progress from show_state. React renders those results and forwards prepare/move intents. Use existing timecode display formatting, components, icons and English/Spanish i18n. No new dependencies, audio scheduling or archive fields.

Validation: focused backend tests cover repeated/omitted/aliased entries, exact delays, hold counts and current/next progress including completion. Run frontend typecheck/tests/build and Rust format, clippy and relevant tests. Inspect representative rendered states when browser access permits; record access failures honestly. Full keyboard, contrast and hardware acceptance testing is outside this approved pass.
