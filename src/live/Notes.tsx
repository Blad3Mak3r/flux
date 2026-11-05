import { useT } from "../i18n";
import { traceStyle } from "../timeline/palette";
import { useLive } from "./store";

/** The notes that are on show now, written for the operator to read from a distance. */
export function Notes() {
  const t = useT();
  const notes = useLive((live) => live.state?.notes);
  const cues = useLive((live) => live.cues);

  return (
    <section className="live-notes" aria-labelledby="live-notes-legend">
      <h2 id="live-notes-legend" className="legend">
        {t("live.notes")}
      </h2>
      {notes && notes.length > 0 ? (
        <ul className="live-notes-list" aria-live="polite">
          {notes.map((note) => {
            const cue = cues.find((candidate) => candidate.id === note.cueId);
            return (
              <li key={note.cueId} className="live-note" style={traceStyle(cue?.color ?? undefined)}>
                <span className="live-number">{cue?.number}</span>
                <p className="live-note-text">{note.text}</p>
                {note.remaining && (
                  <span className="live-note-left measure">{t("live.notes.left", { time: note.remaining })}</span>
                )}
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="live-quiet">{t("live.notes.none")}</p>
      )}
    </section>
  );
}
