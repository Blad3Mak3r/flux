import { VoicePage } from "../voice/VoicePanel";
import { usePreferences } from "../preferences/store";
import { useEffect, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import { ChevronRightIcon, CloseIcon } from "../components/icons";
import type { Devices } from "../devices/useDevices";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { closeSetup, openSetup, useUi } from "../state/ui";
import { useTransport } from "../transport/store";
import { SETUP_GROUPS, groupOf, isAvailable } from "./outputs";
import type { SetupGroup, SetupSection } from "./outputs";
import { ControlPage } from "./ControlPage";
import { AudioOutputsPage, AudioPage, GeneralPage, GraphicsPage, LtcPage, MetronomePage, MtcPage, OscPage, ProjectPage, TimelinePage } from "./pages";

function Page({ section, devices }: { section: SetupSection; devices: Devices }) {
  switch (section) {
    case "operator":
      return <VoicePage devices={devices} />;
    case "timeline":
      return <TimelinePage />;
    case "project":
      return <ProjectPage />;
    case "audio":
      return <AudioPage devices={devices} />;
    case "audioOutputs":
      return <AudioOutputsPage devices={devices} />;
    case "metronome":
      return <MetronomePage devices={devices} />;
    case "ltc":
      return <LtcPage devices={devices} />;
    case "mtc":
      return <MtcPage devices={devices} />;
    case "osc":
      return <OscPage />;
    case "controlMidi":
      return <ControlPage part="midi" />;
    case "controlOsc":
      return <ControlPage part="osc" />;
    case "graphics":
      return <GraphicsPage />;
    case "general":
      return <GeneralPage devices={devices} />;
    default:
      return null;
  }
}

/** What the output is doing, so a change made here can be seen to take effect. */
function Footer() {
  const t = useT();
  const info = useTransport((transport) => transport.info);
  const localError = usePreferences((p) => p.error);
  const pending = useTransport((transport) => transport.pendingReconfigure);
  const position = useTransport((transport) => transport.position);
  const lost = !!position && (position.deviceLost || position.ltcLost !== 0 || position.mtcLost !== 0 || position.metronomeLost);
  const clipWarnings = useTransport((transport) => transport.clipWarnings);
  const warnings = [...(info?.warnings ?? []), ...clipWarnings];
  return (
    <footer className="setup-foot" role="status">
      {localError && <p className="error" role="alert">{t("setup.preferencesError", { error: localError })}</p>}
      <p>{info ? t("out.runningOn", { device: info.device, rate: info.sampleRate }) : t("setup.status.off")}</p>
      {pending && <p>{t("setup.status.pending")}</p>}
      {lost && <p className="warning">{t("setup.status.lost")}</p>}
      {warnings.map((warning) => (
        <p key={warning} className="warning">
          {warning}
        </p>
      ))}
    </footer>
  );
}

const COLLAPSED_KEY = "flux.setup.collapsed";

/** The groups the user folded, remembered between sessions; failing soft to all open. */
function readFolded(): SetupGroup[] {
  try {
    const stored = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]");
    return SETUP_GROUPS.map((group) => group.id).filter((id) => Array.isArray(stored) && stored.includes(id));
  } catch {
    return [];
  }
}

function writeFolded(folded: SetupGroup[]) {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(folded));
  } catch {
    // Nothing to do: the groups open again next time.
  }
}

/** Shorter names for the list where the group already says the rest ("LTC" under Outputs). */
const NAV_LABEL: Partial<Record<SetupSection, MessageKey>> = {
  audio: "setup.nav.audio",
  audioOutputs: "setup.nav.audioOutputs",
  ltc: "setup.nav.ltc",
  mtc: "setup.nav.mtc",
  osc: "setup.nav.osc",
  controlMidi: "setup.nav.controlMidi",
  controlOsc: "setup.nav.osc",
  http: "setup.nav.http",
};

const SOON_HINT: Partial<Record<SetupSection, MessageKey>> = { video: "setup.soon.video", http: "setup.soon.http" };

/**
 * The page list: groups that fold, each a header and its pages. Up and Down walk the focus over
 * what is shown, Home and End jump to the ends, Left folds a group (or goes to its header) and
 * Right opens it. Enter or a click chooses a page.
 */
function SetupNav({ section }: { section: SetupSection }) {
  const t = useT();
  const [folded, setFolded] = useState(readFolded);
  const current = groupOf(section);

  const setGroup = (id: SetupGroup, open: boolean) =>
    setFolded((was) => {
      const next = open ? was.filter((g) => g !== id) : was.includes(id) ? was : [...was, id];
      writeFolded(next);
      return next;
    });

  // A page opened from elsewhere (a faceplate lamp, the show check) shows where it lives.
  useEffect(() => {
    setFolded((was) => (was.includes(current) ? was.filter((g) => g !== current) : was));
  }, [section, current]);

  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const items = Array.from(event.currentTarget.querySelectorAll<HTMLElement>("[data-nav]"));
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    const target = items[at];
    const group = target.dataset.group as SetupGroup;
    let next: HTMLElement | undefined;
    if (event.key === "ArrowDown") next = items[(at + 1) % items.length];
    else if (event.key === "ArrowUp") next = items[(at - 1 + items.length) % items.length];
    else if (event.key === "Home") next = items[0];
    else if (event.key === "End") next = items[items.length - 1];
    else if (event.key === "ArrowLeft") {
      if (target.dataset.nav === "group") setGroup(group, false);
      else next = items.find((item) => item.dataset.nav === "group" && item.dataset.group === group);
    } else if (event.key === "ArrowRight" && target.dataset.nav === "group") setGroup(group, true);
    else return;
    event.preventDefault();
    next?.focus();
  };

  return (
    <nav className="setup-nav" aria-label={t("setup.sections")} onKeyDown={onKeyDown}>
      {SETUP_GROUPS.map((group) => {
        const open = !folded.includes(group.id);
        const holdsPage = group.id === current;
        return (
          <div key={group.id} className="setup-group">
            <button
              type="button"
              className="setup-group-head"
              data-nav="group"
              data-group={group.id}
              aria-expanded={open}
              aria-controls={`setup-group-${group.id}`}
              tabIndex={holdsPage && !open ? 0 : -1}
              onClick={() => setGroup(group.id, !open)}
            >
              <ChevronRightIcon />
              <span>{t(`setup.group.${group.id}` as MessageKey)}</span>
              {holdsPage && !open && <span className="setup-group-lamp" aria-hidden="true" />}
            </button>
            <ul id={`setup-group-${group.id}`} className="setup-group-pages" hidden={!open}>
              {group.sections.map((id) => {
                const available = isAvailable(id);
                const selected = section === id;
                const hint = SOON_HINT[id];
                return (
                  <li key={id}>
                    <button
                      type="button"
                      id={`setup-tab-${id}`}
                      data-nav="page"
                      data-group={group.id}
                      aria-current={selected ? "page" : undefined}
                      aria-disabled={available ? undefined : true}
                      aria-describedby={hint ? `setup-soon-${id}` : undefined}
                      title={hint ? t(hint) : undefined}
                      tabIndex={selected ? 0 : -1}
                      onClick={() => available && openSetup(id)}
                    >
                      <span>{t(NAV_LABEL[id] ?? (`setup.section.${id}` as MessageKey))}</span>
                      {!available && <span className="tag">{t("setup.soon")}</span>}
                      {hint && (
                        <span id={`setup-soon-${id}`} hidden>
                          {t(hint)}
                        </span>
                      )}
                    </button>
                  </li>
                );
              })}
            </ul>
          </div>
        );
      })}
    </nav>
  );
}

/**
 * Everything that is set up rather than played: the project, the audio device and the timecode
 * outputs. A modal: it is rare, it holds lists that would crowd the inspector, and while it is
 * open the transport keys must not fire behind it. The faceplate lamps keep the live state.
 */
export function SetupDialog({ devices }: { devices: Devices }) {
  const t = useT();
  const section = useUi((ui) => ui.setup);
  const ref = useRef<HTMLDialogElement>(null);
  const open = section !== null;

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      dialog.showModal();
      // Land on the page that is open, so the arrow keys walk the list at once.
      dialog.querySelector<HTMLElement>('.setup-nav [tabindex="0"]')?.focus();
    }
    if (!open && dialog.open) dialog.close();
  }, [open]);

  return (
    <dialog ref={ref} className="dialog setup" onCancel={closeSetup} aria-labelledby="setup-title">
      {section && (
        <>
          <header className="setup-head">
            <h2 id="setup-title">{t("setup.title")}</h2>
            <button type="button" className="icon-button" aria-label={t("setup.close")} title={t("setup.close")} onClick={closeSetup}>
              <CloseIcon />
            </button>
          </header>
          <div className="setup-body">
            <SetupNav section={section} />
            <section id="setup-page" className="setup-page" aria-labelledby="setup-page-title">
              <h3 id="setup-page-title">{t(`setup.section.${section}` as MessageKey)}</h3>
              <Page section={section} devices={devices} />
            </section>
          </div>
          <Footer />
        </>
      )}
    </dialog>
  );
}
