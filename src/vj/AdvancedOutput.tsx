// The Advanced Output of a VJ show, as on a media server: the screens with their slices and masks on
// the left, the composition (Input selection) or one screen (Output transformation) in the middle,
// and what is selected on the right. The backend keeps the mapping and every number in bounds; this
// shows the live composition and a drag until it is let go.

import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useT } from "../i18n";
import { SelectField, TextField } from "../components/fields";
import { CloseIcon, EyeIcon, EyeOffIcon, MaskIcon, PlusIcon, ScreenIcon, SliceIcon } from "../components/icons";
import { useDoc } from "../state/store";
import { DEFAULT_SIZE } from "../setup/MappingEditor";
import { MaskFields, SliceFields } from "../setup/SliceFields";
import { OFF, displayLabel, screenNote, useOutputStatus } from "../setup/VideoPage";
import { MAIN_SCREEN, compositionSet, maskEdit, mappingEdit, screenEdit, showTestPattern, useComposition } from "../video/mapping";
import { ScreenSwitch } from "../video/ScreenSwitch";
import type { Mask, Size, Slice, VideoScreen } from "../video/mapping";
import { TestCardFields } from "./TestCardFields";
import { closeOutput } from "./menus";
import { useVj } from "./store";
import { useLivePicture } from "./OutputPreview";
import { Stages } from "./OutputStages";
import type { Item, Stage } from "./OutputStages";

const NO_SCREENS: VideoScreen[] = [];

const NO_SLICES: Slice[] = [];

const NO_MASKS: Mask[] = [];

/** Opens with the Output menu; closes with its button or Escape. */
export function AdvancedOutput() {
  const open = useVj((vj) => vj.output);
  return open ? <OutputDialog /> : null;
}

function OutputDialog() {
  const t = useT();
  const ref = useRef<HTMLDialogElement>(null);
  const { status, choose } = useOutputStatus();
  const screens = useDoc((doc) => doc.project.output.screens ?? NO_SCREENS);
  const composition = useComposition();
  const [stage, setStage] = useState<Stage>("input");
  const [screenId, setScreenId] = useState(MAIN_SCREEN);
  const live = useLivePicture(screenId);
  const [item, setItem] = useState<Item>({ kind: "screen" });
  const [point, setPoint] = useState(0);
  const [pattern, setPattern] = useState(false);

  useEffect(() => {
    ref.current?.showModal();
    return () => void showTestPattern(false);
  }, []);

  const listed = status?.screens ?? [];
  const current = listed.find((s) => s.id === screenId) ?? listed[0];
  const screen = screens.find((s) => s.id === current?.id);
  const slices = screen?.slices ?? NO_SLICES;
  const masks = screen?.masks ?? NO_MASKS;
  const slice = item.kind === "slice" ? slices.find((s) => s.id === item.id) : undefined;
  const mask = item.kind === "mask" ? masks.find((m) => m.id === item.id) : undefined;
  const chosen = current?.chosen ?? OFF;
  const display: Size = status?.displays.find((d) => d.id === chosen) ?? DEFAULT_SIZE;
  const picture: Size = { width: composition.width, height: composition.height };

  const select = (screen: string, next: Item) => {
    setScreenId(screen);
    setItem(next);
    if (next.kind === "mask") setStage("output");
  };
  /** The last slice or mask of `screen` once an add has landed, selected. */
  const selectAdded = (screen: string, kind: "slice" | "mask") => {
    const found = useDoc.getState().project.output.screens?.find((s) => s.id === screen);
    const list = kind === "slice" ? found?.slices : found?.masks;
    const added = list?.[list.length - 1];
    if (added) select(screen, { kind, id: added.id });
  };

  return (
    <dialog ref={ref} className="vj-ao" aria-labelledby="vj-ao-title" onCancel={closeOutput}>
      <header className="vj-ao-head">
        <h2 id="vj-ao-title">{t("vj.ao.title")}</h2>
        <div className="vj-segs" role="tablist" aria-label={t("vj.ao.stage")}>
          {(["input", "output"] as const).map((id) => (
            <button key={id} type="button" role="tab" aria-selected={stage === id} className={stage === id ? "on" : undefined} onClick={() => setStage(id)}>
              {t(id === "input" ? "vj.ao.input" : "vj.ao.output")}
            </button>
          ))}
        </div>
        <div className="vj-ao-comp">
          <span>{t("setup.video.composition")}</span>
          <TextField
            label={t("setup.video.field.width")}
            inputMode="numeric"
            monospace
            value={String(composition.width)}
            onCommit={(width) => void compositionSet(width, String(composition.height))}
          />
          <span aria-hidden="true">×</span>
          <TextField
            label={t("setup.video.field.height")}
            inputMode="numeric"
            monospace
            value={String(composition.height)}
            onCommit={(height) => void compositionSet(String(composition.width), height)}
          />
        </div>
        <button
          type="button"
          className="button"
          aria-pressed={pattern}
          title={t("vj.ao.patternNote")}
          onClick={() => {
            setPattern(!pattern);
            void showTestPattern(!pattern);
          }}
        >
          {t("setup.video.pattern")}
        </button>
        <button type="button" className="icon-button" aria-label={t("setup.close")} title={t("setup.close")} onClick={closeOutput}>
          <CloseIcon />
        </button>
      </header>
      <div className="vj-ao-body">
        <nav className="vj-ao-tree" aria-label={t("setup.video.screens")}>
          <header className="vj-ao-ph">
            <h3>{t("setup.video.screens")}</h3>
            <button
              type="button"
              className="vj-tb"
              onClick={() =>
                void screenEdit({ op: "add" }).then(() => {
                  const list = useDoc.getState().project.output.screens ?? [];
                  const added = list[list.length - 1];
                  if (added) select(added.id, { kind: "screen" });
                })
              }
            >
              <PlusIcon />
              {t("vj.ao.screen")}
            </button>
          </header>
          {!status && <p className="vj-empty">{t("setup.video.loading")}</p>}
          <ul className="vj-ao-screens">
            {listed.map((s) => {
              const own = screens.find((x) => x.id === s.id);
              const on = s.id === current?.id;
              return (
                <li key={s.id} data-off={own?.enabled === false}>
                  <div className="vj-ao-item vj-ao-head">
                    <ScreenSwitch id={s.id} />
                    <button
                      type="button"
                      className="vj-ao-row vj-ao-screen"
                      aria-current={on && item.kind === "screen" ? "true" : undefined}
                      data-open={s.open}
                      onClick={() => select(s.id, { kind: "screen" })}
                    >
                      <ScreenIcon />
                      <span className="vj-ao-name">{s.name}</span>
                      <span className="screen-dot" title={screenNote(t, s)} />
                    </button>
                  </div>
                  <ul>
                    {(own?.slices ?? NO_SLICES).map((x) => (
                      <TreeRow
                        key={x.id}
                        icon={<SliceIcon />}
                        name={x.name}
                        hidden={x.hidden ?? false}
                        selected={on && slice?.id === x.id}
                        onSelect={() => select(s.id, { kind: "slice", id: x.id })}
                        onHide={() => void mappingEdit(s.id, { op: "setHidden", id: x.id, hidden: !x.hidden })}
                      />
                    ))}
                    {(own?.masks ?? NO_MASKS).map((x) => (
                      <TreeRow
                        key={x.id}
                        icon={<MaskIcon />}
                        name={x.name}
                        hidden={x.hidden ?? false}
                        selected={on && mask?.id === x.id}
                        onSelect={() => select(s.id, { kind: "mask", id: x.id })}
                        onHide={() => void maskEdit(s.id, { op: "setHidden", id: x.id, hidden: !x.hidden })}
                      />
                    ))}
                  </ul>
                  {on && (
                    <div className="vj-ao-adds">
                      <button type="button" className="vj-tb" onClick={() => void mappingEdit(s.id, { op: "add" }).then(() => selectAdded(s.id, "slice"))}>
                        <PlusIcon />
                        {t("vj.ao.slice")}
                      </button>
                      <button type="button" className="vj-tb" onClick={() => void maskEdit(s.id, { op: "add" }).then(() => selectAdded(s.id, "mask"))}>
                        <PlusIcon />
                        {t("vj.ao.mask")}
                      </button>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </nav>
        <section className="vj-ao-stage" aria-label={t(stage === "input" ? "vj.ao.input" : "vj.ao.output")}>
          {current && (
            <Stages
              key={`${current.id}-${stage}`}
              stage={stage}
              screenId={current.id}
              slices={slices}
              masks={masks}
              picture={picture}
              display={display}
              live={live}
              slice={slice}
              mask={mask}
              point={point}
              onSelect={(next) => select(current.id, next)}
              onPoint={setPoint}
            />
          )}
          <p className="vj-ao-hint">
            {stage === "input"
              ? t(slices.length ? "vj.ao.inputHint" : "vj.ao.noSlices")
              : t("vj.ao.outputHint", { width: display.width, height: display.height })}
          </p>
        </section>
        <aside className="vj-ao-props" aria-label={t("vj.ao.properties")}>
          {current && slice && (
            <>
              <SliceFields screenId={current.id} slice={slice} picture={picture} display={display} />
              <TestCardFields screenId={current.id} slice={slice} index={Math.max(0, slices.filter((s) => !s.hidden).findIndex((s) => s.id === slice.id))} />
            </>
          )}
          {current && mask && (
            <MaskFields screenId={current.id} mask={mask} point={point} setPoint={setPoint} onRemoved={() => setItem({ kind: "screen" })} />
          )}
          {current && !slice && !mask && (
            <div className="mapping-slice">
              <TextField label={t("setup.video.screenName")} value={current.name} onCommit={(name) => void screenEdit({ op: "rename", id: current.id, name })} />
              <div className="field">
                <label htmlFor="screen-output">{t("vj.ao.screenOutput")}</label>
                <ScreenSwitch id={current.id} switchId="screen-output" />
              </div>
              <SelectField label={t("setup.video.display")} value={chosen} hint={t("setup.video.mainNote")} onChange={(id) => void choose(current.id, id)}>
                <option value={OFF}>{t("setup.video.off")}</option>
                {status?.displays.map((d, index) => (
                  <option key={d.id} value={d.id}>
                    {displayLabel(t, d, index)}
                  </option>
                ))}
                {current.missing && <option value={chosen}>{t("setup.video.disconnected")}</option>}
              </SelectField>
              <p className="field-note" role="status">
                {screenNote(t, current)}
              </p>
              <dl className="vj-meta">
                <div>
                  <dt>{t("vj.ao.slices")}</dt>
                  <dd className="vj-num">{slices.length}</dd>
                </div>
                <div>
                  <dt>{t("vj.ao.masks")}</dt>
                  <dd className="vj-num">{masks.length}</dd>
                </div>
              </dl>
              {status?.decoding && (
                <p className="field-note">
                  {status.decoding === "cpu" ? t("setup.video.decodingCpu") : t("setup.video.decodingGpu", { device: status.decoding })}
                </p>
              )}
              {current.id !== MAIN_SCREEN && (
                <div className="button-row">
                  <button
                    type="button"
                    className="button danger"
                    onClick={() => {
                      select(MAIN_SCREEN, { kind: "screen" });
                      void screenEdit({ op: "remove", id: current.id });
                    }}
                  >
                    {t("setup.video.removeScreen")}
                  </button>
                </div>
              )}
            </div>
          )}
        </aside>
      </div>
    </dialog>
  );
}

function TreeRow({
  icon,
  name,
  hidden,
  selected,
  onSelect,
  onHide,
}: {
  icon: ReactNode;
  name: string;
  hidden: boolean;
  selected: boolean;
  onSelect: () => void;
  onHide: () => void;
}) {
  const t = useT();
  return (
    <li className="vj-ao-item" data-hidden={hidden}>
      <button type="button" className="vj-ao-row" aria-current={selected ? "true" : undefined} onClick={onSelect}>
        {icon}
        <span className="vj-ao-name">{name}</span>
      </button>
      <button
        type="button"
        className="vj-ao-eye"
        aria-pressed={!hidden}
        aria-label={t(hidden ? "vj.ao.show" : "setup.video.hide")}
        title={t(hidden ? "vj.ao.show" : "setup.video.hide")}
        onClick={onHide}
      >
        {hidden ? <EyeOffIcon /> : <EyeIcon />}
      </button>
    </li>
  );
}
