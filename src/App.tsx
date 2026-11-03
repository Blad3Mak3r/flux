import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Faceplate } from "./components/Faceplate";
import { Inspector } from "./components/Inspector";
import type { InspectorTab } from "./components/Inspector";
import { MenuBar } from "./components/MenuBar";
import { StatusBar } from "./components/StatusBar";
import type { Message } from "./components/StatusBar";
import { Timeline } from "./components/Timeline";
import type { Selected } from "./components/TrackRow";
import { UnsavedDialog } from "./components/UnsavedDialog";
import { hasNativeWindow } from "./components/WindowControls";
import { useDevices } from "./devices/useDevices";
import { I18nProvider, useT } from "./i18n";
import type { MessageKey } from "./i18n";
import { errorMessage, importAudio, newProject, openProject, saveProject } from "./project/api";
import { demoShow } from "./state/demo";
import { docReducer, initialDoc, isDirty } from "./state/doc";
import { addAudioClip, addMarker, emptyProject, moveItem, removeItem, toggleMute } from "./state/edit";
import { nominalFps } from "./timecode";
import { patchReadout } from "./timeline/layout";
import { DEFAULT_PX_PER_FRAME, clampZoom } from "./timeline/scale";
import { positionStore, usePlayback } from "./transport/playback";
import type { Project } from "./project/types";

const ZOOM_STEP = 1.25;

const basename = (path: string) => path.split(/[\\/]/).pop() ?? path;

function isTyping(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    (target.isContentEditable || ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName))
  );
}

function Shell() {
  const t = useT();
  const [doc, dispatch] = useReducer(docReducer, undefined, () =>
    initialDoc(emptyProject(t("project.untitled"), "fps25")),
  );
  const { project } = doc;
  const dirty = isDirty(doc);
  const { frameRate, start } = project.timecode;

  const [selection, setSelection] = useState<Selected | null>(null);
  const [tab, setTab] = useState<InspectorTab>("selection");
  const [zoom, setZoom] = useState(DEFAULT_PX_PER_FRAME);
  // A new document starts the timeline over: scrolled to the origin, at the default zoom.
  const [docKey, setDocKey] = useState(0);
  const [message, setMessage] = useState<Message>({ key: "status.ready", tone: "info" });
  const [pending, setPending] = useState<null | (() => Promise<void> | void)>(null);

  const say = useCallback(
    (key: MessageKey, values?: Record<string, string | number>) => setMessage({ key, values, tone: "info" }),
    [],
  );
  const sayError = useCallback((text: string) => setMessage({ text, tone: "error" }), []);
  const fail = useCallback((err: unknown) => sayError(errorMessage(err)), [sayError]);

  const playback = usePlayback(project, sayError);
  const devices = useDevices(project.output);

  // The backend forgets the previous project's assets on `project_new`; start from a clean slate.
  useEffect(() => {
    newProject(project.name, frameRate).catch(() => undefined);
    // Once, on startup.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    document.title = `${project.name}${dirty ? " ●" : ""} — ${t("app.title")}`;
  }, [project.name, dirty, t]);

  // Closing the window (our button, Alt+F4, the OS) must not throw unsaved work away silently.
  const dirtyRef = useRef(dirty);
  dirtyRef.current = dirty;
  useEffect(() => {
    if (!hasNativeWindow()) return;
    const appWindow = getCurrentWindow();
    let stop: (() => void) | undefined;
    let cancelled = false;
    appWindow
      .onCloseRequested((event) => {
        if (!dirtyRef.current) return;
        event.preventDefault();
        setPending(() => () => appWindow.destroy());
      })
      .then((off) => (cancelled ? off() : (stop = off)))
      .catch(() => undefined);
    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  const edit = useCallback((next: Project) => dispatch({ type: "edit", project: next }), []);
  const currentFrame = () => positionStore.get()?.frame ?? playback.parked;

  // Replace the document, asking first when there is work to lose.
  const replaceDoc = (action: () => Promise<void> | void) => (dirty ? setPending(() => action) : void action());

  const load = (next: Project, path: string | null, extra?: { demo?: boolean; durations?: Record<string, number> }) => {
    dispatch({ type: "load", project: next, path, ...extra });
    setSelection(null);
    setZoom(DEFAULT_PX_PER_FRAME);
    setDocKey((key) => key + 1);
    playback.seek(next.timecode.start);
  };

  const doNew = () =>
    replaceDoc(async () => {
      const fresh = await newProject(t("project.untitled"), "fps25").catch(() =>
        emptyProject(t("project.untitled"), "fps25"),
      );
      load(fresh, null);
      say("status.ready");
    });

  const doOpen = () =>
    replaceDoc(async () => {
      try {
        const opened = await openProject();
        if (!opened) return;
        load(opened.project, opened.path);
        say("status.opened", { name: basename(opened.path) });
      } catch (err) {
        fail(err);
      }
    });

  const doDemo = () =>
    replaceDoc(() => {
      const demo = demoShow("Demo show");
      load(demo.project, null, { demo: true, durations: demo.durations });
      say("status.demoLoaded");
    });

  const save = async (asNew: boolean): Promise<boolean> => {
    if (doc.demo) {
      sayError(t("status.demoLoaded"));
      return false;
    }
    try {
      const path = await saveProject(project, asNew ? null : doc.path);
      if (path === null) return false;
      dispatch({ type: "saved", path });
      say("status.saved", { name: basename(path) });
      return true;
    } catch (err) {
      fail(err);
      return false;
    }
  };

  const doImport = async () => {
    try {
      const assets = await importAudio();
      if (assets.length === 0) return;
      let next = project;
      let last = "";
      for (const asset of assets) {
        const added = addAudioClip(next, asset, currentFrame() - start);
        next = added.project;
        last = added.id;
        say("status.imported", { name: asset.name });
      }
      edit(next);
      setSelection({ kind: "clip", id: last });
      setTab("selection");
    } catch (err) {
      fail(err);
    }
  };

  const doMarker = () => {
    const added = addMarker(project, currentFrame() - start, (n) => `Marker ${n}`, t("track.kind.events"));
    edit(added.project);
    setSelection({ kind: "trigger", id: added.id });
    setTab("selection");
    say("status.markerAdded");
  };

  const doDelete = () => {
    if (!selection) return;
    edit(removeItem(project, selection.id));
    setSelection(null);
  };

  const select = (next: Selected | null) => {
    setSelection(next);
    if (next) setTab("selection");
  };

  // Keyboard first: the keys a show operator reaches for without looking.
  const handlers = useRef({ doNew, doOpen, save, doImport, doMarker, doDelete, playback, currentFrame });
  handlers.current = { doNew, doOpen, save, doImport, doMarker, doDelete, playback, currentFrame };
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (pending || isTyping(event.target)) return;
      const h = handlers.current;
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      const fps = nominalFps(frameRate);
      const seekBy = (frames: number) => h.playback.seek(h.currentFrame() + frames);

      if (mod) {
        const action =
          key === "z" ? (event.shiftKey ? "redo" : "undo")
          : key === "y" ? "redo"
          : key === "s" ? (event.shiftKey ? "saveAs" : "save")
          : key === "o" ? "open"
          : key === "n" ? "new"
          : key === "=" || key === "+" ? "zoomIn"
          : key === "-" ? "zoomOut"
          : null;
        if (!action) return;
        event.preventDefault();
        if (action === "undo" || action === "redo") dispatch({ type: action });
        else if (action === "save") void h.save(false);
        else if (action === "saveAs") void h.save(true);
        else if (action === "open") h.doOpen();
        else if (action === "new") h.doNew();
        else setZoom((z) => clampZoom(action === "zoomIn" ? z * ZOOM_STEP : z / ZOOM_STEP));
        return;
      }

      if (event.key === " ") {
        event.preventDefault();
        void h.playback.toggle();
      } else if (event.key === "Home") {
        event.preventDefault();
        void h.playback.stop();
      } else if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
        event.preventDefault();
        seekBy((event.key === "ArrowLeft" ? -1 : 1) * (event.shiftKey ? fps : 1));
      } else if (key === "m") {
        h.doMarker();
      } else if (key === "i") {
        void h.doImport();
      } else if (event.key === "Delete" || event.key === "Backspace") {
        h.doDelete();
      } else if (event.key === "Escape") {
        patchReadout({ spanA: null, spanB: null });
        setSelection(null);
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
  }, [pending, frameRate]);

  const closeDialog = () => setPending(null);

  return (
    <div className="app">
      <MenuBar
        name={project.name}
        dirty={dirty}
        canUndo={doc.past.length > 0}
        canRedo={doc.future.length > 0}
        onNew={doNew}
        onOpen={doOpen}
        onSave={() => void save(false)}
        onSaveAs={() => void save(true)}
        onImport={() => void doImport()}
        onDemo={doDemo}
        onUndo={() => dispatch({ type: "undo" })}
        onRedo={() => dispatch({ type: "redo" })}
      />
      <Faceplate project={project} playback={playback} devices={devices} />
      <main className="work">
        <Timeline
          key={docKey}
          project={project}
          durations={doc.durations}
          parked={playback.parked}
          selection={selection}
          pxPerFrame={zoom}
          zoomPercent={Math.round((zoom / DEFAULT_PX_PER_FRAME) * 100)}
          onZoom={(z) => setZoom(clampZoom(z))}
          onSelect={select}
          onMove={(id, position) => edit(moveItem(project, id, position))}
          onToggleMute={(trackId) => edit(toggleMute(project, trackId))}
          onSeek={playback.seek}
        />
        <Inspector
          tab={tab}
          onTab={setTab}
          project={project}
          selection={selection}
          devices={devices}
          playback={playback}
          onEdit={edit}
          onDelete={doDelete}
        />
      </main>
      <StatusBar message={message} rate={frameRate} start={start} />
      <UnsavedDialog
        name={project.name}
        open={pending !== null}
        onSave={async () => {
          const action = pending;
          if (action && (await save(false))) {
            closeDialog();
            await action();
          }
        }}
        onDiscard={async () => {
          const action = pending;
          closeDialog();
          await action?.();
        }}
        onCancel={closeDialog}
      />
    </div>
  );
}

export default function App() {
  return (
    <I18nProvider>
      <Shell />
    </I18nProvider>
  );
}
