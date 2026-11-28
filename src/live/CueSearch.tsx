import { useEffect, useId, useState } from "react";
import type { KeyboardEvent } from "react";
import { useT } from "../i18n";
import { useDoc } from "../state/store";
import { seek } from "../transport/playback";
import { liveSearch } from "./api";
import { cueName } from "./names";
import { useLive } from "./store";

/**
 * Finds a cue by number or name and goes to it. Which cues match is the backend's answer; this
 * only types the query, lists the ids it returns and seeks like a double click on the cue's row.
 */
export function CueSearch() {
  const t = useT();
  const listId = useId();
  const project = useDoc((doc) => doc.project);
  const cues = useLive((live) => live.cues);
  const [query, setQuery] = useState("");
  const [ids, setIds] = useState<string[]>([]);
  const [active, setActive] = useState(0);
  const typed = query.trim() !== "";

  useEffect(() => {
    if (!typed) {
      setIds([]);
      return;
    }
    let stale = false;
    liveSearch(project, query).then(
      (found) => {
        if (stale) return;
        setIds(found);
        setActive(0);
      },
      () => !stale && setIds([]),
    );
    return () => {
      stale = true;
    };
  }, [project, query, typed]);

  const found = ids.flatMap((id) => cues.find((cue) => cue.id === id) ?? []);

  const go = (frame: number) => {
    seek(frame);
    setQuery("");
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((at) => Math.min(Math.max(at + step, 0), Math.max(found.length - 1, 0)));
    } else if (event.key === "Enter") {
      event.preventDefault();
      const cue = found[active];
      if (cue) go(cue.frame);
    } else if (event.key === "Escape" && typed) {
      event.preventDefault();
      event.stopPropagation();
      setQuery("");
    }
  };

  return (
    <div className="live-search">
      <input
        type="search"
        role="combobox"
        aria-expanded={typed}
        aria-controls={listId}
        aria-label={t("live.search")}
        placeholder={t("live.search")}
        value={query}
        spellCheck={false}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={onKeyDown}
      />
      {typed && (
        <ul id={listId} className="live-search-results" role="listbox" aria-label={t("live.search.results")}>
          {found.length === 0 ? (
            <li className="live-search-none">{t("live.search.none")}</li>
          ) : (
            found.map((cue, index) => (
              <li key={cue.id} role="option" aria-selected={index === active}>
                <button
                  type="button"
                  tabIndex={-1}
                  data-active={index === active}
                  onMouseEnter={() => setActive(index)}
                  onClick={() => go(cue.frame)}
                >
                  <span className="measure">{cue.number}</span>
                  <span className="live-search-name">{cueName(cue, t)}</span>
                  <span className="measure">{cue.timecode}</span>
                </button>
              </li>
            ))
          )}
        </ul>
      )}
    </div>
  );
}
