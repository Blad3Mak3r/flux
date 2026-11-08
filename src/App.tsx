import { ShowCheckDialog } from "./voice/ShowCheckDialog";
import { ErrorDialog } from "./components/ErrorDialog";
import { bindVoice, protectVoiceWork } from "./voice/store";
import { useOutputView } from "./preferences/outputs";
import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { errorMessage, projectClose } from "./project/api";
import { Faceplate } from "./components/Faceplate";
import { MenuHost } from "./components/Menu";
import { textMenu } from "./components/menus";
import { Inspector } from "./components/Inspector";
import { MenuBar } from "./components/MenuBar";
import { StatusBar } from "./components/StatusBar";
import { Timeline } from "./components/Timeline";
import { Live } from "./live/Live";
import { bindRemoteControl } from "./control/bind";
import { bindLive } from "./live/feed";
import { useLive } from "./live/store";
import { TimelineConfirm } from "./components/TimelineConfirm";
import { UnsavedDialog } from "./components/UnsavedDialog";
import { hasNativeWindow } from "./components/WindowControls";
import { useDevices } from "./devices/useDevices";
import type { Devices } from "./devices/useDevices";
import { SetupDialog } from "./setup/SetupDialog";
import { I18nProvider, useT } from "./i18n";
import { deleteSelection, renameSelectedTrack } from "./state/actions";
import { splitAtPlayhead } from "./state/clips";
import { activeTimeline, isDirty } from "./state/doc";
import { bindDocumentSync, flushDocumentSync } from "./state/persist";
import { dispatch, useDoc } from "./state/store";
import { ask, closeMenu, openMenu, openSetup, select, sayError, setView, toggleInspector, toggleSnap, useUi, zoomBy } from "./state/ui";
import {
  addNoteAt,
  closeDocument,
  importAudioFiles,
  newDocument,
  openDocument,
  saveDocument,
  startDocument,
} from "./state/workflows";
import type { EditorPhase } from "./state/workflows";
import { bindTimelineAdvance, stepTimeline } from "./state/timelines";
import { nominalFps } from "./timecode";
import { patchReadout } from "./timeline/layout";
import { bindTransport, seek, stop, toggle } from "./transport/playback";
import { currentFrame, useTransport } from "./transport/store";

const ZOOM_STEP = 1.25;

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName))
  );
}

/**
 * The frame around the panels: it wires the window to the stores and holds nothing the panels
 * draw, so an edit, a selection or a playhead move re-renders the panels that show it, not this.
 */
function Shell({ devices }: { devices: Devices }) {
  const t = useT();
  const name = useDoc((doc) => doc.project.name);
  const dirty = useDoc(isDirty);
  const docKey = useUi((ui) => ui.docKey);
  const collapsed = useUi((ui) => ui.collapsed);
  const view = useUi((ui) => ui.view);
  const asking = useUi((ui) => ui.pending !== null);
  const clockUnpaced = useTransport((transport) => transport.position?.clockUnpaced ?? false);

  // The clock of the audio device is the master; one that does not keep real time ruins the timecode.
  useEffect(() => {
    if (clockUnpaced) sayError(t("output.clockFast"));
  }, [clockUnpaced, t]);

  useEffect(() => {
    document.title = `${name}${dirty ? " ●" : ""} — ${t("app.title")}`;
  }, [name, dirty, t]);

  // A right click nothing claimed (our menus stop theirs): close the open menu, and offer the text
  // menu on a text field. The webview's own menu is blocked for every window in main.tsx.
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (event.defaultPrevented) return;
      closeMenu();
      const field = event.target;
      if (field instanceof HTMLElement && field.matches('textarea, input:not([type]), input[type="text"], input[type="search"]')) {
        field.focus();
        openMenu({ x: event.clientX, y: event.clientY, items: textMenu(t, field as HTMLInputElement) });
      }
    };
    document.addEventListener("contextmenu", onContextMenu);
    return () => document.removeEventListener("contextmenu", onContextMenu);
  }, [t]);

  // Keyboard first: the keys a show operator reaches for without looking.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // A dialog that is open owns the keyboard; the transport keys must not fire behind it.
      const ui = useUi.getState();
      if (ui.errors.length || ui.pending || ui.setup || ui.showCheck || ui.removing !== null || isTyping(event.target)) return;
      if (event.key === " " && event.target instanceof Element && event.target.closest("button, summary, [role='button']")) return;
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      const fps = nominalFps(activeTimeline(useDoc.getState()).timecode.frameRate);
      const seekBy = (frames: number) => seek(currentFrame() + frames);

      if (mod) {
        const action =
          key === "z" ? (event.shiftKey ? "redo" : "undo")
          : key === "y" ? "redo"
          : key === "s" ? (event.shiftKey ? "saveAs" : "save")
          : key === "o" ? "open"
          : key === "n" ? "new"
          : key === "w" ? "close"
          : key === "b" ? "toggleInspector"
          : key === "," ? "setup"
          : key === "pageup" ? "previousTimeline"
          : key === "pagedown" ? "nextTimeline"
          : key === "1" ? "viewEdit"
          : key === "2" ? "viewLive"
          : key === "=" || key === "+" ? "zoomIn"
          : key === "-" ? "zoomOut"
          : null;
        if (!action) return;
        event.preventDefault();
        if (action === "undo" || action === "redo") dispatch({ type: action });
        else if (action === "save") void saveDocument(false);
        else if (action === "saveAs") void saveDocument(true);
        else if (action === "open") openDocument();
        else if (action === "new") newDocument(t);
        else if (action === "close") closeDocument();
        else if (action === "toggleInspector") toggleInspector();
        else if (action === "setup") openSetup();
        else if (action === "previousTimeline" || action === "nextTimeline") {
          void stepTimeline(action === "previousTimeline" ? -1 : 1);
        } else if (action === "viewEdit" || action === "viewLive") setView(action === "viewEdit" ? "edit" : "live");
        else zoomBy(action === "zoomIn" ? ZOOM_STEP : 1 / ZOOM_STEP);
        return;
      }

      if (event.key === " ") {
        event.preventDefault();
        void toggle();
      } else if (event.key === "Home") {
        event.preventDefault();
        void stop();
      } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        seekBy((event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? fps : 1));
      } else if (key === "n") {
        // Live only reads the show; notes are written in the editor.
        if (useUi.getState().view !== "edit") return;
        event.preventDefault();
        addNoteAt(t);
      } else if (key === "s") {
        // Like notes, clips are cut in the editor only.
        if (useUi.getState().view !== "edit") return;
        event.preventDefault();
        void splitAtPlayhead();
      } else if (key === "g") {
        // The magnet is for placing things: Live only reads the show.
        if (useUi.getState().view !== "edit") return;
        event.preventDefault();
        toggleSnap();
      } else if (event.key === "F2") {
        // Names are typed in the editor; Live only reads the show.
        if (useUi.getState().view !== "edit") return;
        event.preventDefault();
        renameSelectedTrack();
      } else if (key === "i") {
        void importAudioFiles();
      } else if (event.key === "Delete" || event.key === "Backspace") {
        deleteSelection();
      } else if (event.key === "Escape") {
        patchReadout({ spanA: null, spanB: null });
        select(null);
      }
    };
    // Space activates a focused button on key-up; keep it from also firing a click.
    const onKeyUp = (event: KeyboardEvent) => {
      if (event.key === " " && !isTyping(event.target)) event.preventDefault();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [t]);

  return (
    <div className="app">
      <MenuBar />
      <Faceplate devices={devices} />
      <main className="work" data-view={view} data-inspector-collapsed={collapsed}>
        {view === "live" ? (
          <Live />
        ) : (
          <>
            <Timeline key={docKey} />
            <Inspector />
          </>
        )}
      </main>
      <StatusBar />
      <SetupDialog devices={devices} />
      <ShowCheckDialog />
      <MenuHost />
      <TimelineConfirm />
      <UnsavedDialog
        name={name}
        open={asking}
        onSave={async () => {
          const action = useUi.getState().pending;
          if (action && (await saveDocument(false))) {
            ask(null);
            await action();
          }
        }}
        onDiscard={async () => {
          const action = useUi.getState().pending;
          ask(null);
          await action?.();
        }}
        onCancel={() => ask(null)}
      />
    </div>
  );
}

function Editor() {
  const t = useT();
  const native = hasNativeWindow();
  const [phase, setPhase] = useState<EditorPhase>("project");
  const [connected, setConnected] = useState(!native);
  const [ready, setReady] = useState(!native);
  const output = useOutputView();
  const view = useUi((ui) => ui.view);
  const devices = useDevices(output, !native || phase === "outputs");
  const liveEnabled = connected && view === "live";
  const cuesReady = useLive((live) => live.ready);
  const waiting = !connected || devices.loading || (liveEnabled && !cuesReady);

  useEffect(bindTransport, []);
  useEffect(bindVoice, []);
  useEffect(bindDocumentSync, []);
  useEffect(() => {
    if (!native) return;
    const listening = listen("recovery-error", () => sayError(t("autosave.failed")));
    return () => void listening.then((off) => off()).catch(() => undefined);
  }, [native, t]);
  useEffect(bindTimelineAdvance, []);
  useEffect(bindRemoteControl, []);
  useEffect(() => {
    if (native) void startDocument(t, setPhase).then(() => setConnected(true));
  }, [native, t]);
  useEffect(() => {
    if (liveEnabled) return bindLive();
  }, [liveEnabled]);
  useEffect(() => {
    if (!waiting) setReady(true);
  }, [waiting]);

  // The OS can request a close before the editor's controls are mounted.
  useEffect(() => {
    if (!native) return;
    const appWindow = getCurrentWindow();
    const listening = appWindow.onCloseRequested((event) => {
      event.preventDefault();
      const close = async () => {
        try {
          await flushDocumentSync();
          await projectClose();
          await appWindow.destroy();
        } catch (err) { sayError(errorMessage(err)); }
      };
      void protectVoiceWork().then((safe) => { if (!safe) return; if (isDirty(useDoc.getState())) ask(close); else void close(); });
    }).catch(() => undefined);
    return () => void listening.then((off) => off?.());
  }, [native]);

  if (ready) return <Shell devices={devices} />;
  const step = connected && !devices.loading && liveEnabled ? "cues" : phase;
  return (
    <main className="editor-loading">
      <section className="editor-loading-panel" aria-labelledby="editor-loading-title">
        <h1 id="editor-loading-title">{t("editor.loading.title")}</h1>
        <p role="status">{t(`editor.loading.${step}`)}</p>
        <progress aria-labelledby="editor-loading-title" />
      </section>
    </main>
  );
}

export default function App() {
  return (
    <I18nProvider>
      <Editor />
      <ErrorDialog />
    </I18nProvider>
  );
}
