// The effects of the VJ screen: the stack of a clip or a layer in the inspector, and the browser that lists every
// effect to drag onto one. The backend owns the kinds, the settings with their ranges and defaults, the order and
// the limit; this draws what its catalogue says and forwards the user's intent.

import { Fragment, useState } from "react";
import type { DragEvent, ReactNode } from "react";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  ChevronDownIcon,
  ChevronRightIcon,
  CloseIcon,
  DuplicateIcon,
  EyeIcon,
  EyeOffIcon,
  GripIcon,
  RefreshIcon,
} from "../components/icons";
import { useT } from "../i18n";
import { effectModulation } from "../project/types";
import type { MessageKey } from "../i18n";
import type { VjEffect, VjEffectKind, VjGrid, VjModulations, VjParamValue } from "../project/types";
import type { VjEffectSpec, VjParamSpec } from "./api";
import { ColourField, Scrub, Segments } from "./controls";
import { categoryKey, choiceKey, effectKey, genericParamKey, paramKey, specOf, useEffectCatalog } from "./effects";
import { ModPanel, ModScrub, beatLabel } from "./Modulation";
import type { ModRange } from "./Modulation";
import { edit, foldEffect, preview, showTab, useVj } from "./store";

/** What an effect dragged from the browser carries: the name of its kind. */
export const DRAG_EFFECT = "application/x-flux-vj-effect";
/** Carried too by an effect that only works on the composition, so a drag over a clip can tell. */
const DRAG_COMPOSITION = "application/x-flux-vj-effect-composition";
/** What an effect row dragged within a stack carries: its id. */
const DRAG_ROW = "application/x-flux-vj-fx";

/** What an effect is added to: a clip, a layer or the composition. */
export type EffectTarget = { clip: string } | { layer: string } | "composition";

/** Whether a drag carries an effect from the browser that `target` takes: one for the composition only is not for a clip or a layer. */
export const carriesEffect = (event: DragEvent, target: EffectTarget) =>
  event.dataTransfer.types.includes(DRAG_EFFECT) && (target === "composition" || !event.dataTransfer.types.includes(DRAG_COMPOSITION));

/** Adds the effect a drop carries to `target`; whether it carried one that `target` takes. */
export function dropEffect(event: DragEvent, target: EffectTarget): boolean {
  const kind = event.dataTransfer.getData(DRAG_EFFECT);
  if (!kind || !carriesEffect(event, target)) return false;
  event.preventDefault();
  event.stopPropagation();
  void edit({ op: "addEffect", target, kind });
  if (target === "composition") showTab("composition");
  return true;
}

/** What makes an element take the effects dropped on it into the composition (the output panel, the composition bar). */
export function useCompositionDrop() {
  const [over, setOver] = useState(false);
  return {
    over,
    props: {
      onDragOver: (event: DragEvent) => {
        if (!carriesEffect(event, "composition")) return;
        event.preventDefault();
        setOver(true);
      },
      onDragLeave: () => setOver(false),
      onDrop: (event: DragEvent) => {
        setOver(false);
        dropEffect(event, "composition");
      },
    },
  };
}

/**
 * The composition's effects at a glance: how many there are, which opens their tab, and a switch that bypasses them
 * all at once, which is for the stage. Nothing while there are none.
 */
export function CompositionFx({ grid }: { grid: VjGrid }) {
  const t = useT();
  const count = grid.compositionEffects?.length ?? 0;
  const bypassed = grid.compositionBypass === true;
  if (count === 0) return null;
  return (
    <span className={`vj-cfx${bypassed ? " off" : ""}`}>
      <button type="button" className="vj-fxb" title={t("vj.composition.badge", { n: count })} onClick={() => showTab("composition")}>FX {count}</button>
      <button type="button" className="vj-fx-b" aria-pressed={bypassed} title={t(bypassed ? "vj.composition.bypassed" : "vj.composition.bypass")}
        aria-label={t("vj.composition.bypass")} onClick={() => void edit({ op: "compositionBypass", bypass: !bypassed })}>
        {bypassed ? <EyeOffIcon /> : <EyeIcon />}
      </button>
    </span>
  );
}

/** The count of effects drawn on a cell or a layer header. */
export function EffectBadge({ count }: { count: number }) {
  const t = useT();
  if (count === 0) return null;
  return <span className="vj-fxb" title={t("vj.effect.badge", { n: count })}>FX {count}</span>;
}


type Changed = Record<string, VjParamValue>;

/** The settings of one effect, each drawn as its catalogue entry says. */
function Settings({ id, spec, kind, modulation }: { id: string; spec: VjEffectSpec; kind: VjEffectKind; modulation: VjModulations | undefined }) {
  const t = useT();
  const withValues = (changed: Changed) => ({ ...kind, ...changed }) as VjEffectKind;
  const move = (changed: Changed) => preview({ op: "effectParams", effect: id, kind: withValues(changed) });
  const keep = (changed: Changed) => void edit({ op: "effectParams", effect: id, kind: withValues(changed) });
  const label = (key: string) => t(paramKey(spec.name, key)) ?? t(genericParamKey(key));

  type NumberParam = Extract<VjParamSpec, { type: "number" }>;
  const rangeOf = (param: NumberParam): ModRange => {
    const percent = param.unit === "%";
    return { min: param.min, max: param.max, shown: percent ? 100 : 1, unit: param.unit, decimals: percent || param.whole ? 0 : param.step >= 0.1 ? 1 : 2 };
  };
  const scrub = (param: NumberParam, tag: string) => {
    const range = rangeOf(param);
    const shared = {
      label: tag, name: label(param.key), value: kind[param.key] as number, shown: range.shown, reset: param.default, unit: param.unit,
      decimals: range.decimals, perPixel: (param.max - param.min) / 160, step: param.whole ? 1 : param.step,
      onMove: (v: number) => move({ [param.key]: v }), onSet: (v: number) => keep({ [param.key]: v }),
    };
    if (!param.modulatable) return <Scrub key={param.key} {...shared} />;
    return <ModScrub key={param.key} {...shared} target={{ effect: id, param: param.key }} modulation={modulation?.[param.key]} range={range} />;
  };
  // The sub-row of a modulated setting, under the row that holds it.
  const panel = (param: VjParamSpec) =>
    param.type === "number" && param.modulatable
      ? <ModPanel key={`m-${param.key}`} target={{ effect: id, param: param.key }} modulation={modulation?.[param.key]} range={rangeOf(param)} />
      : null;
  const control = (param: VjParamSpec): ReactNode => {
    switch (param.type) {
      case "number":
        if (param.snap.length > 0) {
          const options = param.snap.map(String);
          return (
            <Segments options={options} value={String(kind[param.key])} label={(o) => beatLabel(Number(o))}
              onPick={(o) => keep({ [param.key]: param.snap[options.indexOf(o)] })} />
          );
        }
        return <div className="vj-scrubs one">{scrub(param, "↔")}</div>;
      case "choice":
        return (
          <Segments options={param.options} value={kind[param.key] as string} label={(o) => t(choiceKey(param.key, o))}
            onPick={(o) => keep({ [param.key]: o })} />
        );
      case "colour":
        return (
          <ColourField value={kind[param.key] as string} label={label(param.key)}
            onMove={(colour) => move({ [param.key]: colour })} onSet={(colour) => keep({ [param.key]: colour })} />
        );
      case "flag": {
        const on = kind[param.key] === true;
        return (
          <button type="button" role="switch" aria-checked={on} className={`vj-sw${on ? " on" : ""}`}
            aria-label={label(param.key)} onClick={() => keep({ [param.key]: !on })}>
            <i />
          </button>
        );
      }
    }
  };

  const rows: ReactNode[] = [];
  const params = spec.params;
  for (let at = 0; at < params.length; at++) {
    const param = params[at];
    // An effect that is free counts in Hz and one that is synced in beats: only the one that applies shows.
    if (spec.animated && ((param.key === "rate" && kind.sync !== "free") || (param.key === "beats" && kind.sync !== "beats"))) continue;
    const next = params[at + 1];
    const pair =
      param.type === "number" && next?.type === "number" &&
      ((param.key === "x" && next.key === "y") || (param.key === "columns" && next.key === "rows"));
    if (pair) {
      const [first, second] = param.key === "x" ? ["X", "Y"] : ["C", "R"];
      rows.push(
        <Fragment key={param.key}>
          <div className="vj-f"><span>{t(paramKey(spec.name, param.key + next.key)) ?? t(genericParamKey(param.key + next.key))}</span>
            <div className="vj-scrubs">{scrub(param, first)}{scrub(next, second)}</div>
          </div>
          {panel(param)}{panel(next)}
        </Fragment>,
      );
      at++;
      continue;
    }
    rows.push(<Fragment key={param.key}><div className="vj-f"><span>{label(param.key)}</span>{control(param)}</div>{panel(param)}</Fragment>);
  }
  return <div className="vj-fx-p">{rows}</div>;
}

/** One effect of a stack: its name, fold, bypass, copy, reset, order and removal, and its settings. */
function EffectRow({ effect, index, stack, full }: { effect: VjEffect; index: number; stack: VjEffect[]; full: boolean }) {
  const t = useT();
  const catalog = useEffectCatalog((state) => state.catalog);
  const folded = useVj((vj) => vj.folded[effect.id] === true);
  const [edge, setEdge] = useState<"before" | "after" | null>(null);
  // The modulation stays out of the settings the backend takes back with the new values.
  const { id, bypass, modulation: _modulation, ...rest } = effect as typeof effect & { modulation?: unknown };
  const kind = rest as VjEffectKind;
  const spec = specOf(catalog, kind.effect);
  const count = stack.length;
  const name = t(effectKey(kind.effect));
  const button = (action: MessageKey, run: () => void, icon: ReactNode, disabled = false, pressed?: boolean) => {
    const label = `${t(action)}: ${name}`;
    return <button type="button" className="vj-fx-b" disabled={disabled} aria-pressed={pressed} title={label} aria-label={label} onClick={run}>{icon}</button>;
  };
  // A row dragged here goes before or after this one, by which half it is dropped on.
  const over = (event: DragEvent<HTMLLIElement>) => {
    if (!event.dataTransfer.types.includes(DRAG_ROW)) return;
    event.preventDefault();
    const box = event.currentTarget.getBoundingClientRect();
    setEdge(event.clientY < box.top + box.height / 2 ? "before" : "after");
  };
  const drop = (event: DragEvent<HTMLLIElement>) => {
    const from = event.dataTransfer.getData(DRAG_ROW);
    const at = stack.findIndex((e) => e.id === from);
    setEdge(null);
    if (!from || at < 0 || from === id) return;
    event.preventDefault();
    event.stopPropagation();
    const place = index + (edge === "after" ? 1 : 0);
    void edit({ op: "moveEffect", effect: from, index: at < place ? place - 1 : place });
  };
  return (
    <li className={`vj-fx-item${bypass ? " off" : ""}${folded ? " folded" : ""}${edge ? ` drop-${edge}` : ""}`}
      onDragOver={over} onDragLeave={() => setEdge(null)} onDrop={drop}>
      <div className="vj-fx-h">
        <span className="vj-fx-g" draggable title={t("vj.effect.drag")} aria-label={t("vj.effect.drag")}
          onDragStart={(event) => {
            event.dataTransfer.setData(DRAG_ROW, id);
            event.dataTransfer.effectAllowed = "move";
            const row = event.currentTarget.closest("li");
            if (row) event.dataTransfer.setDragImage(row, 12, 12);
          }}><GripIcon /></span>
        <button type="button" className="vj-fx-n" aria-expanded={!folded} title={t(folded ? "vj.effect.open" : "vj.effect.fold")}
          onClick={() => foldEffect(id)}>
          {folded ? <ChevronRightIcon /> : <ChevronDownIcon />}
          <span>{name}</span>
        </button>
        {button("vj.effect.bypass", () => void edit({ op: "effectBypass", effect: id, bypass: !bypass }), bypass ? <EyeOffIcon /> : <EyeIcon />, false, !!bypass)}
        {button("vj.effect.duplicate", () => void edit({ op: "duplicateEffect", effect: id }), <DuplicateIcon />, full)}
        {button("vj.effect.reset", () => void edit({ op: "resetEffect", effect: id }), <RefreshIcon />)}
        {button("vj.effect.up", () => void edit({ op: "moveEffect", effect: id, index: index - 1 }), <ArrowUpIcon />, index === 0)}
        {button("vj.effect.down", () => void edit({ op: "moveEffect", effect: id, index: index + 1 }), <ArrowDownIcon />, index === count - 1)}
        {button("vj.effect.remove", () => void edit({ op: "removeEffect", effect: id }), <CloseIcon />)}
      </div>
      {!folded && spec && <Settings id={id} spec={spec} kind={kind} modulation={effectModulation(effect)} />}
    </li>
  );
}

/** The effects stack of a clip or a layer, applied from the top down; the backend orders, limits and draws it. */
export function EffectStack({ target, effects }: { target: EffectTarget; effects: VjEffect[] }) {
  const t = useT();
  const catalog = useEffectCatalog((state) => state.catalog);
  const [over, setOver] = useState(false);
  const max = catalog?.max ?? effects.length;
  const full = effects.length >= max;
  const composition = target === "composition";
  const title = t(composition ? "vj.composition.effects" : "vj.effects");
  return (
    <section className={`vj-fx${over ? " over" : ""}`} aria-label={title}
      onDragOver={(event) => {
        if (!carriesEffect(event, target)) return;
        event.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(event) => {
        setOver(false);
        dropEffect(event, target);
      }}>
      <header className="vj-fx-head">
        <h3>{title}<span className="vj-dim vj-fx-count">{effects.length}/{catalog?.max ?? "–"}</span></h3>
        <select className="vj-select wide vj-fx-add" value="" disabled={full || !catalog} aria-label={t("vj.effect.add")}
          title={full ? t("vj.effect.full", { n: max }) : undefined}
          onChange={(event) => { if (event.target.value) void edit({ op: "addEffect", target, kind: event.target.value }); }}>
          <option value="">{t("vj.effect.add")}</option>
          {catalog?.categories.filter((category) => composition || catalog.effects.some((spec) => spec.category === category && !spec.compositionOnly)).map((category) => (
            <optgroup key={category} label={t(categoryKey(category))}>
              {catalog.effects.filter((spec) => spec.category === category && (composition || !spec.compositionOnly)).map((spec) => (
                <option key={spec.name} value={spec.name}>{t(effectKey(spec.name))}</option>
              ))}
            </optgroup>
          ))}
        </select>
      </header>
      {effects.length === 0 ? <p className="vj-dim vj-fx-empty">{t(composition ? "vj.composition.empty" : "vj.effect.empty")}</p> : (
        <ol className="vj-fx-list">
          {effects.map((effect, index) => <EffectRow key={effect.id} effect={effect} index={index} stack={effects} full={full} />)}
        </ol>
      )}
      {full && <p className="vj-dim vj-fx-empty">{t("vj.effect.full", { n: max })}</p>}
    </section>
  );
}

/** Every effect by category: drag one onto a clip, a layer, the output or an inspector stack, or double click it to add it to `target`. */
export function EffectBrowser({ target }: { target: EffectTarget | null }) {
  const t = useT();
  const catalog = useEffectCatalog((state) => state.catalog);
  if (!catalog) return <p className="vj-empty">{t("vj.effect.loading")}</p>;
  // An effect for the composition only goes there, whatever is selected, and its tab opens.
  const add = (spec: VjEffectSpec) => {
    const to = spec.compositionOnly ? "composition" : target;
    if (!to) return;
    void edit({ op: "addEffect", target: to, kind: spec.name });
    if (to === "composition") showTab("composition");
  };
  return (
    <div className="vj-fxl">
      {catalog.categories.map((category) => (
        <section key={category} className="vj-fxs">
          <h3>{t(categoryKey(category))}</h3>
          <div className="vj-fxg">
            {catalog.effects.filter((spec) => spec.category === category).map((spec) => (
              <div key={spec.name} className="vj-fxc" draggable role="button" tabIndex={0}
                title={t("vj.effects.note")}
                onDragStart={(event) => {
                  event.dataTransfer.setData(DRAG_EFFECT, spec.name);
                  if (spec.compositionOnly) event.dataTransfer.setData(DRAG_COMPOSITION, spec.name);
                  event.dataTransfer.effectAllowed = "copy";
                }}
                onDoubleClick={() => add(spec)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") add(spec);
                }}>
                <b>{t(effectKey(spec.name))}
                  {spec.animated && <i className="vj-fxt" title={t("vj.effect.animated")}>↻</i>}
                  {spec.compositionOnly && <i className="vj-fxo" title={t("vj.composition.onlyHint")}>{t("vj.composition.only")}</i>}
                </b>
                <span>{t(effectKey(spec.name, "hint"))}</span>
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
