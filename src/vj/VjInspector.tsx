// The inspector of a VJ show: clip, layer, group, composition, dashboard and mappings.

import { Fragment, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { Marquee } from "../components/Marquee";
import { useT } from "../i18n";
import { MediaFile } from "../media/MediaFile";
import type { VideoFit, VjBlend, VjClip, VjGenerator, VjEffect, VjGrid, VjPlayback, VjTrigger } from "../project/types";
import { useDoc } from "../state/store";
import { edit, bindModulation, preview, useVj } from "./store";
import { round2 } from "../format";
import { layerName } from "./names";
import { Dashboard } from "./Dashboard";
import { mappable } from "./mapModel";
import { MappingsPanel } from "./Mappings";
import { ColourField, Scrub, Segments } from "./controls";
import { EffectStack } from "./EffectStack";
import { useEffectCatalog } from "./effects";
import { GeneratorModPanel, GeneratorScrub, ModPanel, ModScrub, fieldRange, inspectorModulated } from "./Modulation";
import { TextFields } from "./TextFields";
import { LiveInputFields } from "./LiveInputFields";
import { Transport } from "./Transport";
import { Select } from "../components/Select";
import { clock, stripOf, Thumb } from "./VideoThumb";
import { NO_VIDEOS, AUTOPILOTS } from "./shared";
import { blendOptions, transitionOptions, transitionValue, transitionEdit } from "./LayerOptions";

const NO_EFFECTS: VjEffect[] = [];

const PLAYBACKS: VjPlayback[] = ["loop", "once", "pingPong"];

const TRIGGERS: VjTrigger[] = ["normal", "piano"];

const FITS: VideoFit[] = ["fit", "fill", "stretch", "none"];

/** A name typed in place; one the backend refuses goes back to the name it had. */
function NameField({ value, placeholder, label, onCommit }: { value: string; placeholder: string; label: string; onCommit: (name: string) => Promise<boolean> }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <input className="vj-input" value={text} placeholder={placeholder} aria-label={label}
      onChange={(event) => setText(event.target.value)}
      onBlur={() => {
        if (text !== value) void onCommit(text).then((done) => { if (!done) setText(value); });
      }}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Enter") event.currentTarget.blur();
        if (event.key === "Escape") { setText(value); event.currentTarget.blur(); }
      }} />
  );
}

/** The parameters of a generator clip, each with the backend's default where the clip has none. */
function GeneratorFields({ clip, generator, modulation }: { clip: string; generator: VjGenerator; modulation: VjClip["modulation"] }) {
  const t = useT();
  const move = (changed: Partial<VjGenerator>) =>
    preview({ op: "clipGenerator", clip, generator: { ...generator, ...changed } as VjGenerator });
  const keep = (changed: Partial<VjGenerator>) =>
    void edit({ op: "clipGenerator", clip, generator: { ...generator, ...changed } as VjGenerator });
  const colour = (key: "colour" | "from" | "to", value: string, name: string) => (
    <div className="vj-f"><span>{name}</span>
      <ColourField value={value} label={name} onMove={(c) => move({ [key]: c })} onSet={(c) => keep({ [key]: c })} />
    </div>
  );
  const number = (key: "rate" | "width" | "angle", value: number, props: { label: string; name: string; shown: number; reset: number; unit: string; decimals?: number; hint?: string }) => (
    <>
      <div className="vj-f" title={props.hint}><span>{props.name}</span>
        <div className="vj-scrubs one">
          <GeneratorScrub {...props} clip={clip} modulation={modulation} field={key} value={value} onMove={(v) => move({ [key]: v })} onSet={(v) => keep({ [key]: v })} />
        </div>
      </div>
      <GeneratorModPanel clip={clip} modulation={modulation} field={key} shown={props.shown} unit={props.unit} decimals={props.decimals} />
    </>
  );
  switch (generator.kind) {
    case "solid":
      return colour("colour", generator.colour ?? "#ffffff", t("vj.gen.colour"));
    case "strobe":
      return (
        <>
          {colour("colour", generator.colour ?? "#ffffff", t("vj.gen.colour"))}
          {number("rate", generator.rate ?? 8, { label: "↻", name: t("vj.gen.rate"), shown: 1, reset: 8, unit: "Hz", decimals: 1, hint: t("vj.gen.rateHint") })}
          {number("width", generator.width ?? 0.5, { label: "▮", name: t("vj.gen.width"), shown: 100, reset: 0.5, unit: "%", hint: t("vj.gen.widthHint") })}
        </>
      );
    case "gradient":
      return (
        <>
          {colour("from", generator.from ?? "#000000", t("vj.gen.from"))}
          {colour("to", generator.to ?? "#ffffff", t("vj.gen.to"))}
          {number("angle", generator.angle ?? 0, { label: "∠", name: t("vj.gen.angle"), shown: 1, reset: 0, unit: "°" })}
        </>
      );
    case "noise":
      return (
        <>
          {colour("colour", generator.colour ?? "#ffffff", t("vj.gen.colour"))}
          {number("rate", generator.rate ?? 30, { label: "↻", name: t("vj.gen.rate"), shown: 1, reset: 30, unit: "Hz", decimals: 1, hint: t("vj.gen.noiseRateHint") })}
        </>
      );
    case "text":
      return <TextFields clip={clip} generator={generator} modulation={modulation} />;
    case "liveInput":
      return <LiveInputFields clip={clip} generator={generator} />;
    case "bars":
      return null;
  }
}

/** The levels of a layer the inspector sets and can modulate: the same as its master, audio and video faders. */
const LEVELS = [
  { field: "master", op: "layerMaster", label: "vj.fader.master", tag: "M" },
  { field: "volume", op: "layerVolume", label: "vj.fader.audio", tag: "A" },
  { field: "opacity", op: "layerOpacity", label: "vj.fader.video", tag: "V" },
] as const;

/** The panels that show what is selected, and the dashboard and the mappings. */
type InspectorTab = "clip" | "layer" | "group" | "composition" | "dashboard" | "mappings";

export function Inspector({ grid, tab }: { grid: VjGrid; tab: InspectorTab }) {
  const t = useT();
  // One value each, so the play state polled twenty times a second does not draw the inspector again.
  const clipId = useVj((vj) => vj.clip);
  const layerId = useVj((vj) => vj.layer);
  const groupId = useVj((vj) => vj.group);
  const videos = useDoc((doc) => doc.project.videos ?? NO_VIDEOS);
  const probes = useDoc((doc) => doc.video);
  const thumbs = useDoc((doc) => doc.thumbs);
  const found = grid.layers.flatMap((layer) => (layer.clips ?? []).map((clip) => ({ layer, clip }))).find((x) => x.clip.id === clipId);
  const layer = grid.layers.find((l) => l.id === layerId) ?? found?.layer;
  const group = grid.groups?.find((g) => g.id === groupId);
  const modCatalog = useEffectCatalog((state) => state.catalog?.modulation);
  let body: ReactNode = <p className="vj-empty">{t(tab === "clip" ? "vj.inspector.noClip" : tab === "group" ? "vj.inspector.noGroup" : "vj.inspector.noLayer")}</p>;
  if (tab === "clip" && found) {
    const { clip } = found;
    const { generator } = clip;
    // A live input has no length: no transport, end action or countdown (the backend reports none).
    const endless = generator?.kind === "liveInput";
    const video = videos.find((v) => v.id === clip.asset);
    const probe = clip.asset ? probes[clip.asset] : undefined;
    const deck = grid.decks.find((d) => d.id === grid.deck);
    const scale = clip.scale ?? 1;
    const [x, y] = clip.position ?? [0, 0];
    const columnAt = found.layer.global ? clip.slot ?? 0 : deck?.columns.findIndex((c) => c.id === clip.column) ?? -1;
    // A transform number that can be modulated, with its "~" button; the row of `scale` is drawn here, the two of
    // `position` by the caller. The sub-rows go under them.
    const modScrub = (field: string, name: string, tag: string, value: number, reset: number, onMove: (v: number) => void, onSet: (v: number) => void, bare = false) => {
      const range = fieldRange(modCatalog, "clip", field);
      const scrub = { label: tag, name, value, shown: 100, reset, onMove, onSet };
      const control = range
        ? <ModScrub key={field} {...scrub} target={{ clip: clip.id, field }} modulation={clip.modulation?.[field]} range={range} />
        : <Scrub key={field} {...scrub} />;
      return bare ? control : <div className="vj-f"><span>{name}</span><div className="vj-scrubs one">{control}</div></div>;
    };
    const modPanel = (field: string) => {
      const range = fieldRange(modCatalog, "clip", field);
      return range && <ModPanel key={field} target={{ clip: clip.id, field }} modulation={clip.modulation?.[field]} range={range} />;
    };
    body = (
      <div className="vj-insp">
        {!endless && <Transport clip={clip} layer={found.layer.id} strip={stripOf(thumbs, clip)} still={probe?.still} />}
        <Thumb className="vj-ithumb" strip={stripOf(thumbs, clip)} generator={generator} />
        <div className="vj-fields">
          <label className="vj-f"><span>{t("vj.clip.name")}</span>
            <NameField value={clip.name ?? ""} placeholder={generator ? t(`vj.generator.${generator.kind}`) : video?.name ?? ""} label={t("vj.clip.name")}
              onCommit={(name) => edit({ op: "renameClip", clip: clip.id, name })} />
          </label>
          <div className="vj-f"><span>{t("vj.clip.trigger")}</span>
            <Segments options={TRIGGERS} value={clip.trigger ?? "normal"} label={(o) => t(`vj.trigger.${o}`)}
              onPick={(trigger) => void edit({ op: "clipTrigger", clip: clip.id, trigger })} />
          </div>
          {generator && <GeneratorFields clip={clip.id} generator={generator} modulation={clip.modulation} />}
          {!endless && (
            <div className="vj-f"><span>{t("vj.clip.playback")}</span>
              <Segments options={PLAYBACKS} value={clip.playback ?? "loop"} label={(o) => t(`vj.playback.${o}`)}
                onPick={(playback) => void edit({ op: "clipPlayback", clip: clip.id, playback })} />
            </div>
          )}
          {!generator && (
            <div className="vj-f"><span>{t("vj.clip.fit")}</span>
              <Segments options={FITS} value={clip.fit ?? "fit"} label={(o) => t(`vj.fit.${o}`)}
                onPick={(fit) => void edit({ op: "clipFit", clip: clip.id, fit })} />
            </div>
          )}
          {modScrub("scale", t("vj.clip.scale"), "↔", scale, 1, (v) => preview({ op: "clipScale", clip: clip.id, value: v }),
            (v) => void edit({ op: "clipScale", clip: clip.id, value: v }))}
          {modPanel("scale")}
          <div className="vj-f"><span>{t("vj.clip.position")}</span>
            <div className="vj-scrubs">
              {modScrub("positionX", t("vj.clip.x"), "X", x, 0, (v) => preview({ op: "clipPosition", clip: clip.id, x: v, y }),
                (v) => void edit({ op: "clipPosition", clip: clip.id, x: v, y }), true)}
              {modScrub("positionY", t("vj.clip.y"), "Y", y, 0, (v) => preview({ op: "clipPosition", clip: clip.id, x, y: v }),
                (v) => void edit({ op: "clipPosition", clip: clip.id, x, y: v }), true)}
            </div>
          </div>
          {(["positionX", "positionY"] as const).map((field) => modPanel(field))}
        </div>
        <EffectStack target={{ clip: clip.id }} effects={clip.effects ?? NO_EFFECTS} />
        <Dashboard key={clip.id} grid={grid} scope={{ kind: "clip", clip: clip.id }} />
        <dl className="vj-meta">
          {generator ? (
            <div><dt>{t("vj.gen.kind")}</dt><dd>{t(`vj.generator.${generator.kind}`)}</dd></div>
          ) : (
            <>
              <div><dt>{t("vj.clip.file")}</dt><dd><Marquee>{video?.name ?? ""}</Marquee></dd></div>
              {video && <MediaFile id={video.id} />}
              <div><dt>{t("vj.clip.length")}</dt><dd className="vj-mono">{!probe ? "—" : probe.still ? t("vj.media.still") : clock(probe.frames * probe.rateDen / probe.rateNum)}</dd></div>
              <div><dt>{t("vj.clip.video")}</dt><dd className="vj-mono">{!probe ? "—" : probe.still ? `${probe.codec} · ${probe.width}×${probe.height}` : `${probe.codec} · ${probe.width}×${probe.height} · ${round2(probe.rateNum / probe.rateDen)}`}</dd></div>
            </>
          )}
          <div><dt>{t("vj.clip.cell")}</dt><dd><Marquee>{layerName(t, grid, found.layer)} · {columnAt + 1}</Marquee></dd></div>
        </dl>
      </div>
    );
  } else if (tab === "layer" && layer) {
    body = (
      <div className="vj-insp single">
        <div className="vj-fields">
          <label className="vj-f"><span>{t("vj.layer.name")}</span>
            <NameField value={layer.name ?? ""} placeholder={t("vj.layer.default", { n: grid.layers.indexOf(layer) + 1 })} label={t("vj.layer.name")}
              onCommit={(name) => edit({ op: "renameLayer", layer: layer.id, name })} />
          </label>
          <label className="vj-f"><span>{t("vj.layer.blend")}</span>
            <Select className="vj-select wide" value={layer.blend ?? "alpha"}
              onChange={(value) => void edit({ op: "layerBlend", layer: layer.id, blend: value as VjBlend })}>
              {blendOptions(t)}
            </Select>
          </label>
          <label className="vj-f"><span>{t("vj.layer.transitionEffect")}</span>
            <Select className="vj-select wide" value={transitionValue(layer)}
              onChange={(value) => void edit(transitionEdit(layer, value))}>
              {transitionOptions(t)}
            </Select>
          </label>
          <div className="vj-f" {...mappable({ kind: "layerAutopilot", layer: layer.id })}><span>{t("vj.layer.autopilot")}</span>
            <Segments options={AUTOPILOTS} value={layer.autopilot ?? "off"} label={(o) => t(`vj.autopilot.${o}`)}
              onPick={(mode) => void edit({ op: "layerAutopilot", layer: layer.id, mode })} />
          </div>
          <div className="vj-f"><span>{t("vj.layer.transition")}</span>
            <span className="vj-mono">{t("vj.seconds", { n: (layer.transition ?? 0).toFixed(1) })}</span>
          </div>
          <div className="vj-f"><span>{t("vj.layer.kind")}</span>
            <span>{t(layer.global ? "vj.layer.kindGlobal" : "vj.layer.kindDeck")}</span>
          </div>
          {LEVELS.map(({ field, op, label, tag }) => {
            const range = fieldRange(modCatalog, "layer", field);
            const value = layer[field] ?? 1;
            const scrub = {
              label: tag, name: t(label), value, shown: 100, reset: 1,
              onMove: (v: number) => preview({ op, layer: layer.id, value: v }),
              onSet: (v: number) => void edit({ op, layer: layer.id, value: v }),
            };
            return (
              <Fragment key={field}>
                <div className="vj-f"><span>{t(label)}</span>
                  <div className="vj-scrubs one">
                    {range
                      ? <ModScrub {...scrub} target={{ layer: layer.id, field }} modulation={layer.modulation?.[field]} range={range} />
                      : <Scrub {...scrub} />}
                  </div>
                </div>
                {range && <ModPanel target={{ layer: layer.id, field }} modulation={layer.modulation?.[field]} range={range} />}
              </Fragment>
            );
          })}
        </div>
        <EffectStack target={{ layer: layer.id }} effects={layer.effects ?? NO_EFFECTS} />
        <Dashboard key={layer.id} grid={grid} scope={{ kind: "layer", layer: layer.id }} />
      </div>
    );
  } else if (tab === "group" && group) {
    body = (
      <div className="vj-insp single">
        <div className="vj-fields">
          <label className="vj-f"><span>{t("vj.group.name")}</span>
            <NameField value={group.name ?? ""} placeholder={t("vj.group.default", { n: (grid.groups ?? []).indexOf(group) + 1 })} label={t("vj.group.name")}
              onCommit={(name) => edit({ op: "renameGroup", group: group.id, name })} />
          </label>
          <label className="vj-f"><span>{t("vj.layer.blend")}</span>
            <Select className="vj-select wide" value={group.blend ?? "alpha"}
              onChange={(value) => void edit({ op: "groupBlend", group: group.id, blend: value as VjBlend })}>
              {blendOptions(t)}
            </Select>
          </label>
          {([["master", "groupMaster", "vj.fader.master", "M"], ["opacity", "groupOpacity", "vj.fader.video", "V"]] as const).map(([field, op, label, tag]) => (
            <div key={field} className="vj-f"><span>{t(label)}</span>
              <div className="vj-scrubs one">
                <Scrub label={tag} name={t(label)} value={group[field] ?? 1} shown={100} reset={1}
                  onMove={(v: number) => preview({ op, group: group.id, value: v })}
                  onSet={(v: number) => void edit({ op, group: group.id, value: v })} />
              </div>
            </div>
          ))}
        </div>
        <p className="vj-dim vj-fx-empty">{t("vj.group.note")}</p>
        <EffectStack target={{ group: group.id }} effects={group.effects ?? NO_EFFECTS} />
      </div>
    );
  } else if (tab === "composition") {
    body = (
      <div className="vj-insp single">
        <p className="vj-dim vj-fx-empty">{t("vj.composition.note")}</p>
        <EffectStack target="composition" effects={grid.compositionEffects ?? NO_EFFECTS} />
      </div>
    );
  } else if (tab === "dashboard") {
    body = (
      <div className="vj-insp single">
        <Dashboard grid={grid} />
      </div>
    );
  } else if (tab === "mappings") {
    body = <MappingsPanel grid={grid} />;
  }
  return <div className="vj-pb">{body}</div>;
}

/** Asks for the modulated values while anything on the screen is modulated: the selection, a layer's faders or a knob. */
export function ModulationBinding({ grid }: { grid: VjGrid }) {
  const clipId = useVj((vj) => vj.clip);
  const layerId = useVj((vj) => vj.layer);
  const clip = grid.layers.flatMap((layer) => layer.clips ?? []).find((c) => c.id === clipId);
  const layer = grid.layers.find((l) => l.id === layerId) ?? grid.layers.find((l) => l.clips?.some((c) => c.id === clipId));
  const modulated =
    inspectorModulated(layer, clip, grid.compositionEffects) || grid.layers.some((l) => inspectorModulated(l, undefined)) || (grid.dashboard ?? []).some((knob) => knob.modulation);
  useEffect(() => (modulated ? bindModulation() : undefined), [modulated]);
  return null;
}
