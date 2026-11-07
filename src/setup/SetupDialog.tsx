import { useEffect, useRef } from "react";
import { CloseIcon } from "../components/icons";
import type { Devices } from "../devices/useDevices";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import { closeSetup, openSetup, useUi } from "../state/ui";
import { useTransport } from "../transport/store";
import { SETUP_SECTIONS, isAvailable, stepSection } from "./outputs";
import type { SetupSection } from "./outputs";
import { ControlPage } from "./ControlPage";
import { AudioPage, GeneralPage, GraphicsPage, LtcPage, MetronomePage, MtcPage, OscPage, ProjectPage, TimelinePage } from "./pages";

function Page({ section, devices }: { section: SetupSection; devices: Devices }) {
  switch (section) {
    case "timeline":
      return <TimelinePage />;
    case "project":
      return <ProjectPage />;
    case "audio":
      return <AudioPage devices={devices} />;
    case "metronome":
      return <MetronomePage devices={devices} />;
    case "ltc":
      return <LtcPage devices={devices} />;
    case "mtc":
      return <MtcPage devices={devices} />;
    case "osc":
      return <OscPage />;
    case "control":
      return <ControlPage />;
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
  const pending = useTransport((transport) => transport.pendingReconfigure);
  const clipWarnings = useTransport((transport) => transport.clipWarnings);
  const warnings = [...(info?.warnings ?? []), ...clipWarnings];
  return (
    <footer className="setup-foot" role="status">
      <p>{info ? t("out.runningOn", { device: info.device, rate: info.sampleRate }) : t("setup.status.off")}</p>
      {pending && <p>{t("setup.status.pending")}</p>}
      {warnings.map((warning) => (
        <p key={warning} className="warning">
          {warning}
        </p>
      ))}
    </footer>
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
      // Land on the page that is open, so the arrow keys move between pages at once.
      dialog.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]')?.focus();
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
            <div className="setup-nav" role="tablist" aria-orientation="vertical" aria-label={t("setup.sections")}>
              {SETUP_SECTIONS.map((id) => {
                const available = isAvailable(id);
                return (
                  <button
                    key={id}
                    type="button"
                    role="tab"
                    id={`setup-tab-${id}`}
                    aria-selected={section === id}
                    aria-controls="setup-page"
                    aria-disabled={available ? undefined : true}
                    tabIndex={section === id ? 0 : -1}
                    onClick={() => available && openSetup(id)}
                    onKeyDown={(event) => {
                      const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
                      if (step === 0) return;
                      event.preventDefault();
                      const next = stepSection(section, step);
                      openSetup(next);
                      document.getElementById(`setup-tab-${next}`)?.focus();
                    }}
                  >
                    <span>{t(`setup.section.${id}` as MessageKey)}</span>
                    {!available && <span className="tag">{t("setup.soon")}</span>}
                  </button>
                );
              })}
            </div>
            <section id="setup-page" className="setup-page" role="tabpanel" aria-labelledby={`setup-tab-${section}`}>
              <h3>{t(`setup.section.${section}` as MessageKey)}</h3>
              <Page section={section} devices={devices} />
            </section>
          </div>
          <Footer />
        </>
      )}
    </dialog>
  );
}
