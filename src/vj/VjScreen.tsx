import { useEffect } from "react";
import type { ReactNode } from "react";
import { useT } from "../i18n";
import type { VjGrid } from "../project/types";
import { useDoc } from "../state/store";
import { bindPlayState, loadLayout, useVj } from "./store";
import type { MonitorKind } from "./api";
import { Dock } from "./Dock";
import { Monitor } from "./Monitor";
import { MapLayer } from "./MapLayer";
import { MapImportDialog } from "./MapImportDialog";
import { LibraryDialog, NameDialog } from "./PresetDialogs";
import { loadPresets } from "./presets";
import { bindVjControl } from "./mapping";
import { bindOutputState } from "./output";
import { bindRecordState } from "./record";
import { AdvancedOutput } from "./AdvancedOutput";
import { loadEffectCatalog } from "./effects";
import "./vj.css";
import { DeckPanel } from "./DeckPanel";
import { Inspector, ModulationBinding } from "./VjInspector";
import { FilesPanel, GeneratorsPanel, EffectsPanel } from "./BrowserPanels";

/** The panel with id `id`; a monitor is `monitor:<uuid>`. */
function panelOf(grid: VjGrid, id: string, monitors: { id: string; kind: MonitorKind }[]): ReactNode {
  switch (id) {
    case "clip": case "layer": case "group": case "composition": case "dashboard": case "mappings":
      return <Inspector grid={grid} tab={id} />;
    case "files": return <FilesPanel grid={grid} />;
    case "generators": return <GeneratorsPanel />;
    case "effects": return <EffectsPanel grid={grid} />;
    default: {
      const monitor = monitors.find((m) => m.id === id);
      return monitor && <Monitor key={id} grid={grid} id={id} kind={monitor.kind} />;
    }
  }
}

/** The screen of a VJ project: the deck, fixed, and the panels docked around it where the layout puts them. */
export function VjScreen() {
  const grid = useDoc((doc) => doc.project.vj);
  const monitors = useVj((vj) => vj.layout?.current.monitors);
  useEffect(bindPlayState, []);
  useEffect(bindVjControl, []);
  useEffect(bindOutputState, []);
  useEffect(bindRecordState, []);
  useEffect(() => { void loadEffectCatalog(); void loadPresets(); void loadLayout(); }, []);
  if (!grid) return null;
  return (
    <div className="vj-app">
      <Dock deck={<DeckPanel grid={grid} />} renderPanel={(id) => panelOf(grid, id, monitors ?? [])} />
      <ModulationBinding grid={grid} />
      <AdvancedOutput />
      <MapLayer grid={grid} />
      <MapImportDialog />
      <NameDialog />
      <LibraryDialog />
    </div>
  );
}

/** The VJ screen's one short tip, at the right of the status bar; the bar shows it only when it has no message of its own, and the stylesheet hides it on narrow windows. */
export function VjTip() {
  const t = useT();
  return <span className="vj-tips">{t("vj.tip")}</span>;
}
