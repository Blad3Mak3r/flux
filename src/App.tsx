import { fail } from "./state/fail";
import { bindShow } from "./show/api";
import { SetlistEditor } from "./show/SetlistEditor";
import { ControlEditor } from "./show/ControlEditor";
import { ShowCheckDialog } from "./voice/ShowCheckDialog";
import { LtcDialog } from "./export/LtcDialog";
import { AboutDialog } from "./components/AboutDialog";
import { ErrorDialog } from "./components/ErrorDialog";
import { bindVoice, protectVoiceWork } from "./voice/store";
import { useOutputView } from "./preferences/outputs";
import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { projectClose } from "./project/api";
import { Faceplate } from "./components/Faceplate";
import { MenuHost } from "./components/Menu";
import { textMenu } from "./components/menus";
import { Inspector } from "./components/Inspector";
import { MenuBar } from "./components/MenuBar";
import { StatusBar } from "./components/StatusBar";
import { Timeline } from "./components/Timeline";
import { Live } from "./live/Live";
import type { Action } from "./keymap/actions";
import { actionOf } from "./keymap/keys";
import { vjKeyDown } from "./vj/menus";
import { forwardKeyDown, forwardKeyUp, releaseKeys } from "./vj/keys";
import { useVj } from "./vj/store";
import { VjScreen } from "./vj/VjScreen";
import { bindRemoteControl } from "./control/bind";
import { MediaDialog } from "./media/MediaDialog";
import { bindMedia, useMedia } from "./media/store";
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
import { watchMediaProgress } from "./media/progress";
import { watchFileDrops } from "./state/fileDrop";
import { bindDocumentSync, flushDocumentSync } from "./state/persist";
import { dispatch, useDoc } from "./state/store";
import { ask, closeAbout, closeMenu, cycleRipple, openMenu, openSetup, say, select, sayError, setView, toggleInspector, toggleSnap, useUi, zoomBy } from "./state/ui";
import { loadShowLock } from "./live/lock";
import { panic } from "./live/panic";
import { isShowLocked } from "./live/lockStore";
import { zoomToFit } from "./state/items";
import { selectAll } from "./state/selection";
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
import { bindTempoMap } from "./state/tempoMap";
import { addMarkerAtPlayhead, addRegionFromSelection, deleteSelectedMark, selectMark, stepMarker } from "./state/marks";
import { currentFrame, useTransport } from "./transport/store";

const ZOOM_STEP = 1.25;

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable ||
      ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName) ||
      target.getAttribute("aria-haspopup") === "listbox")
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
  const editSurface = useUi((ui) => ui.editSurface);
  const controlEditor = useUi((ui) => ui.controlEditor);
  const view = useUi((ui) => ui.view);
  const asking = useUi((ui) => ui.pending !== null);
  const about = useUi((ui) => ui.about);
  const vj = useDoc((doc) => doc.project.mode === "vj");
  const clockUnpaced = useTransport((transport) => transport.position?.clockUnpaced ?? false);

  // The clock of the audio device is the master; one that does not keep real time ruins the timecode.
  useEffect(() => {
    if (clockUnpaced) sayError(t("output.clockFast"));
  }, [clockUnpaced, t]);

  // Files dragged in from the file explorer.
  useEffect(() => {
    const stop = watchFileDrops();
    return () => void stop.then((off) => off());
  }, []);

  useEffect(() => {
    const stop = watchMediaProgress();
    return () => void stop.then((off) => off());
  }, []);

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
      if (ui.errors.length || ui.pending || ui.setup || ui.showCheck || ui.about || ui.removing !== null || useVj.getState().output || useMedia.getState().open || isTyping(event.target)) return;
      // A VJ show has keys of its own, and none of the timeline's.
      if (useDoc.getState().project.mode === "vj") {
        // A key a mapping waits for is the mapping's, not a shortcut's.
        if (forwardKeyDown(event)) {
          event.preventDefault();
          return;
        }
        vjKeyDown(event, t);
        return;
      }
      if (event.key === " " && event.target instanceof Element && event.target.closest("button, summary, [role='button']")) return;
      const found = actionOf(event);
      if (!found) return;
      // Live only follows the show: what places things belongs to the editor.
      if (found.editOnly && useUi.getState().view !== "edit") return;
      event.preventDefault();
      // A locked show keeps to the show: nothing that edits, opens, closes or leaves Live.
      if (found.locked && isShowLocked()) return say("live.lock.blocked");
      const fps = nominalFps(activeTimeline(useDoc.getState()).timecode.frameRate);
      const seekBy = (frames: number) => seek(currentFrame() + frames);
      const run: Record<Action, () => void> = {
        playPause: () => void toggle(),
        stop: () => void stop(),
        frameBack: () => seekBy(-1),
        frameForward: () => seekBy(1),
        secondBack: () => seekBy(-fps),
        secondForward: () => seekBy(fps),
        panic: () => void panic(t),
        new: () => newDocument(t),
        open: openDocument,
        save: () => void saveDocument(false),
        saveAs: () => void saveDocument(true),
        close: closeDocument,
        setup: () => openSetup(),
        importAudio: () => void importAudioFiles(),
        undo: () => dispatch({ type: "undo" }),
        redo: () => dispatch({ type: "redo" }),
        selectAll: selectAll,
        delete: () => {
          if (!deleteSelectedMark()) deleteSelection();
        },
        deselect: () => {
          patchReadout({ spanA: null, spanB: null });
          selectMark(null);
          select(null);
        },
        split: () => void splitAtPlayhead(),
        addNote: () => addNoteAt(t),
        renameTrack: renameSelectedTrack,
        toggleSnap: toggleSnap,
        cycleRipple: cycleRipple,
        addMarker: () => void addMarkerAtPlayhead(t),
        addRegion: () => void addRegionFromSelection(t),
        previousMarker: () => void stepMarker(false),
        nextMarker: () => void stepMarker(true),
        previousTimeline: () => void stepTimeline(-1),
        nextTimeline: () => void stepTimeline(1),
        viewEdit: () => setView("edit"),
        viewLive: () => setView("live"),
        zoomIn: () => zoomBy(ZOOM_STEP),
        zoomOut: () => zoomBy(1 / ZOOM_STEP),
        zoomFit: () => void zoomToFit(),
        toggleInspector: toggleInspector,
      };
      run[found.id]();
    };
    // Space activates a focused button on key-up; keep it from also firing a click.
    const onKeyUp = (event: KeyboardEvent) => {
      forwardKeyUp(event);
      if (event.key === " " && !isTyping(event.target)) event.preventDefault();
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", releaseKeys);
    return () => {
      window.removeEventListener("blur", releaseKeys);
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
  }, [t]);

  return (
    <div className="app" data-vj={vj || undefined}>
      <MenuBar />
      {vj ? null : <Faceplate devices={devices} />}
      <main className="work" data-view={vj ? "vj" : view} data-edit-surface={editSurface} data-inspector-collapsed={collapsed}>
        {vj ? (
          <VjScreen key={docKey} />
        ) : view === "live" ? (
          <Live />
        ) : editSurface === "setlists" ? (
          <SetlistEditor key={docKey} />
        ) : (
          <>
            <Timeline key={docKey} />
            {controlEditor !== null ? <ControlEditor /> : <Inspector />}
          </>
        )}
      </main>
      <StatusBar />
      <SetupDialog devices={devices} />
      <ShowCheckDialog />
      <LtcDialog />
      <MediaDialog />
      {/* Outside the menu bar, whose button rules would strip the dialog's buttons. */}
      {about ? <AboutDialog onClose={closeAbout} /> : null}
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

  useEffect(bindShow, []);
  useEffect(bindTransport, []);
  useEffect(bindTempoMap, []);
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
    if (native) void loadShowLock();
  }, [native]);
  useEffect(bindMedia, []);
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
        } catch (err) { fail(err); }
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
