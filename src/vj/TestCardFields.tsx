// The test card of a slice, as the Advanced Output's inspector edits it: the cabinet size of the LED wall behind the slice, the
// colour the card's palette is made from, which layers show and what the label says. The backend keeps every value in range
// and draws the card (flux-render/src/pattern.rs); this sends the whole settings back with the one change.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { useT } from "../i18n";
import { TextField } from "../components/fields";
import { errorMessage } from "../project/api";
import type { ProjectState } from "../project/types";
import { dispatch, useDoc } from "../state/store";
import { sayError } from "../state/ui";
import { mappingEdit } from "../video/mapping";
import type { CardLayers, CardMode, Slice, TestCard } from "../video/mapping";
import { ColourField, Segments } from "./controls";

const PRESETS = ["custom", "64", "104", "128"] as const;
const NUMBERINGS = ["alpha", "dotted", "sequence"] as const;
const SCHEMES = ["vivid", "dark", "light", "contrast"] as const;
const MODES: CardMode[] = ["pattern", "red", "green", "blue", "white", "black", "custom"];
const LAYERS: (keyof CardLayers)[] = ["grid", "numbers", "circle", "diagonals", "crosshair", "label", "bars", "motion"];
const WIDTHS = ["1", "2", "3", "4", "6"] as const;
const IMAGE_FILES = { name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"] };
const IMAGE_FILE = /\.(png|jpe?g|webp|bmp|gif|tiff?)$/i;
const NO_VIDEOS: never[] = [];
const MODE_SWATCH: Partial<Record<CardMode, string>> = { red: "#ff0000", green: "#00ff00", blue: "#0000ff", white: "#ffffff", black: "#000000" };

export function TestCardFields({ screenId, slice, index }: { screenId: string; slice: Slice; index: number }) {
  const t = useT();
  const card = slice.testCard;
  const send = (next: Partial<TestCard>) => void mappingEdit(screenId, { op: "setTestCard", id: slice.id, card: { ...card, ...next } });
  const layer = (key: keyof CardLayers, on: boolean) => send({ layers: { ...card.layers, [key]: on } });

  // The colour a slice takes when it names none comes from the backend, which owns the palette.
  const [auto, setAuto] = useState("#4da3ff");
  useEffect(() => {
    let live = true;
    void invoke<string>("video_card_base", { index }).then((hex) => live && setAuto(hex), () => undefined);
    return () => {
      live = false;
    };
  }, [index]);

  // The images of the project's media a logo can be.
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const images = videos.filter((video) => IMAGE_FILE.test(video.file));
  const importLogo = async () => {
    try {
      const path = await open({ filters: [IMAGE_FILES] });
      if (typeof path !== "string") return;
      const before = useDoc.getState().project.videos ?? [];
      const state = await invoke<ProjectState>("project_import_image", {
        project: useDoc.getState().project,
        path,
      });
      dispatch({ type: "apply", project: state.project, active: state.active });
      const added = (state.project.videos ?? []).find((video) => !before.some((old) => old.id === video.id));
      if (added) send({ logo: added.id, showLogo: true });
    } catch (err) {
      sayError(errorMessage(err));
    }
  };

  const row = (name: string, control: ReactNode, hint?: string) => (
    <div className="vj-f" title={hint}>
      <span>{name}</span>
      {control}
    </div>
  );
  const size = card.cabinetWidth === card.cabinetHeight ? String(card.cabinetWidth) : "custom";
  const preset = (PRESETS as readonly string[]).includes(size) ? size : "custom";
  const chip = (label: string, on: boolean, toggle: () => void) => (
    <button key={label} type="button" className="tc-chip" aria-pressed={on} onClick={toggle}>
      {label}
    </button>
  );

  return (
    <fieldset className="tc">
      <legend>{t("vj.card.title")}</legend>
      <div className="tc-show">
        {chip(t("vj.card.show"), card.show, () => send({ show: !card.show }))}
        <p className="field-note">{t("vj.card.showNote")}</p>
      </div>
      {row(
        t("vj.card.mode"),
        <div className="tc-modes" role="radiogroup" aria-label={t("vj.card.mode")}>
          {MODES.map((mode) => (
            <button
              key={mode}
              type="button"
              role="radio"
              aria-checked={card.mode === mode}
              className={card.mode === mode ? "on" : undefined}
              title={t(`vj.card.mode.${mode}`)}
              onClick={() => send({ mode })}
            >
              {mode === "pattern" ? (
                <span className="tc-pattern">{t("vj.card.mode.pattern")}</span>
              ) : (
                <i style={{ background: mode === "custom" ? card.custom : MODE_SWATCH[mode] }} />
              )}
            </button>
          ))}
        </div>,
      )}
      {card.mode === "custom" &&
        row(t("vj.card.custom"), <ColourField value={card.custom} label={t("vj.card.custom")} onMove={() => undefined} onSet={(custom) => send({ custom })} />)}
      {card.mode === "pattern" && (
        <>
          {row(
            t("vj.card.cabinet"),
            <Segments
              options={[...PRESETS]}
              value={preset as (typeof PRESETS)[number]}
              label={(o) => (o === "custom" ? t("vj.card.presetCustom") : `${o}`)}
              onPick={(o) => o !== "custom" && send({ cabinetWidth: Number(o), cabinetHeight: Number(o) })}
            />,
            t("vj.card.cabinetHint"),
          )}
          {row(
            t("vj.card.size"),
            <div className="tc-size">
              <TextField label={t("setup.video.field.width")} inputMode="numeric" monospace value={String(card.cabinetWidth)} onCommit={(v) => send({ cabinetWidth: Number(v) || card.cabinetWidth })} />
              <span aria-hidden="true">×</span>
              <TextField label={t("setup.video.field.height")} inputMode="numeric" monospace value={String(card.cabinetHeight)} onCommit={(v) => send({ cabinetHeight: Number(v) || card.cabinetHeight })} />
              <span className="tc-unit">px</span>
            </div>,
          )}
          {row(
            t("vj.card.base"),
            <div className="tc-base">
              <ColourField value={card.base || auto} label={t("vj.card.base")} onMove={() => undefined} onSet={(base) => send({ base })} />
              <button type="button" className="tc-chip" aria-pressed={card.base === ""} title={t("vj.card.autoHint")} onClick={() => send({ base: "" })}>
                {t("vj.card.auto")}
              </button>
            </div>,
          )}
          {row(
            t("vj.card.scheme"),
            <Segments options={[...SCHEMES]} value={card.scheme} label={(o) => t(`vj.card.scheme.${o}`)} onPick={(scheme) => send({ scheme })} />,
          )}
          {row(
            t("vj.card.numbering"),
            <Segments options={[...NUMBERINGS]} value={card.numbering} label={(o) => t(`vj.card.numbering.${o}`)} onPick={(numbering) => send({ numbering })} />,
          )}
          <div className="tc-layers" role="group" aria-label={t("vj.card.layers")}>
            {LAYERS.map((key) => chip(t(`vj.card.layer.${key}`), card.layers[key], () => layer(key, !card.layers[key])))}
          </div>
          {row(
            t("vj.card.guide"),
            <Segments options={[...WIDTHS]} value={String(WIDTHS.includes(String(card.guideWidth) as (typeof WIDTHS)[number]) ? card.guideWidth : 2) as (typeof WIDTHS)[number]} label={(o) => o} onPick={(o) => send({ guideWidth: Number(o) })} />,
            t("vj.card.guideHint"),
          )}
          {card.layers.label && (
            <>
              <div className="tc-layers" role="group" aria-label={t("vj.card.label")}>
                {chip(t("vj.card.showName"), card.showName, () => send({ showName: !card.showName }))}
                {chip(t("vj.card.showResolution"), card.showResolution, () => send({ showResolution: !card.showResolution }))}
                {chip(t("vj.card.plate"), card.labelPlate, () => send({ labelPlate: !card.labelPlate }))}
              </div>
              {row(
                t("vj.card.labelY"),
                <input
                  type="range"
                  className="tc-range"
                  min={0}
                  max={100}
                  value={card.labelY}
                  aria-label={t("vj.card.labelY")}
                  onChange={(event) => send({ labelY: Number(event.target.value) })}
                />,
              )}
            </>
          )}
        </>
      )}
      {card.mode === "pattern" && (
        <>
          <div className="tc-layers" role="group" aria-label={t("vj.card.logo")}>
            {chip(t("vj.card.logoShow"), card.showLogo, () => send({ showLogo: !card.showLogo }))}
          </div>
          {card.showLogo && (
            <>
              {row(
                t("vj.card.logoImage"),
                <div className="tc-base">
                  <select
                    className="tc-select"
                    aria-label={t("vj.card.logoImage")}
                    value={images.some((video) => video.id === card.logo) ? card.logo : ""}
                    onChange={(event) => send({ logo: event.target.value })}
                  >
                    <option value="">{t("vj.card.logoNone")}</option>
                    {images.map((video) => (
                      <option key={video.id} value={video.id}>
                        {video.name}
                      </option>
                    ))}
                  </select>
                  <button type="button" className="tc-chip" onClick={() => void importLogo()}>
                    {t("vj.card.logoImport")}
                  </button>
                </div>,
              )}
              {row(t("vj.card.logoBack"), <ColourField value={card.logoBack} label={t("vj.card.logoBack")} onMove={() => undefined} onSet={(logoBack) => send({ logoBack })} />)}
              {row(
                t("vj.card.logoSize"),
                <input
                  type="range"
                  className="tc-range"
                  min={10}
                  max={60}
                  value={card.logoSize}
                  aria-label={t("vj.card.logoSize")}
                  onChange={(event) => send({ logoSize: Number(event.target.value) })}
                />,
              )}
            </>
          )}
        </>
      )}
      <div className="button-row">
        <button type="button" className="button" title={t("vj.card.copyHint")} onClick={() => void mappingEdit(screenId, { op: "copyTestCard", id: slice.id })}>
          {t("vj.card.copy")}
        </button>
      </div>
    </fieldset>
  );
}
