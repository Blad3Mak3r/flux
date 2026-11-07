import { useId, useRef, useState } from "react";
import gb from "flag-icons/flags/4x3/gb.svg";
import es from "flag-icons/flags/4x3/es.svg";
import { useI18n } from "./index";
import { LOCALES } from "./messages";
import { Field } from "../components/fields";

const flags = { en: gb, es };

/** Flags accompany readable language names; keyboard focus stays inside the native dialog. */
export function LanguageSelect() {
  const { locale, setLocale, t } = useI18n();
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const button = useRef<HTMLButtonElement>(null);
  const options = useRef<HTMLButtonElement[]>([]);
  const listId = useId();
  const selected = LOCALES.find((l) => l.id === locale) ?? LOCALES[0];
  const close = () => {
    setOpen(false);
    button.current?.focus();
  };
  const show = () => {
    const index = LOCALES.findIndex((l) => l.id === locale);
    setActive(index);
    setOpen(true);
    requestAnimationFrame(() => options.current[index]?.focus());
  };
  const option = (l: typeof selected) => (
    <>
      <img
        src={flags[l.id]}
        className="language-flag"
        alt=""
        aria-hidden="true"
      />
      <span lang={l.id}>{l.label}</span>
    </>
  );
  return (
    <Field label={t("menu.language")}>
      {({ id }) => (
        <div
          className="language-select"
          onBlur={(e) => {
            if (!e.currentTarget.contains(e.relatedTarget)) setOpen(false);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape" && open) {
              e.preventDefault();
              e.stopPropagation();
              close();
            }
          }}
        >
          <button
            ref={button}
            id={id}
            type="button"
            className="language-trigger"
            aria-label={`${t("menu.language")}: ${selected.label}`}
            aria-haspopup="listbox"
            aria-expanded={open}
            aria-controls={open ? listId : undefined}
            onClick={() => (open ? close() : show())}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                show();
              }
            }}
          >
            {option(selected)}
            <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
              <path
                d="m4 6 4 4 4-4"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
              />
            </svg>
          </button>
          {open && (
            <div
              id={listId}
              role="listbox"
              aria-labelledby={id}
              className="language-options"
              onKeyDown={(e) => {
                let next = active;
                if (e.key === "ArrowDown") next = (active + 1) % LOCALES.length;
                else if (e.key === "ArrowUp")
                  next = (active + LOCALES.length - 1) % LOCALES.length;
                else if (e.key === "Home") next = 0;
                else if (e.key === "End") next = LOCALES.length - 1;
                else return;
                e.preventDefault();
                setActive(next);
                options.current[next]?.focus();
              }}
            >
              {LOCALES.map((l, i) => (
                <button
                  key={l.id}
                  ref={(node) => {
                    if (node) options.current[i] = node;
                  }}
                  type="button"
                  role="option"
                  aria-selected={l.id === locale}
                  tabIndex={i === active ? 0 : -1}
                  onFocus={() => setActive(i)}
                  onClick={() => {
                    setLocale(l.id);
                    close();
                  }}
                >
                  {option(l)}
                  {l.id === locale && (
                    <svg
                      width="16"
                      height="16"
                      viewBox="0 0 16 16"
                      aria-hidden="true"
                    >
                      <path
                        d="m3 8 3 3 7-7"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="1.5"
                      />
                    </svg>
                  )}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </Field>
  );
}
