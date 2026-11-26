// The effects of the VJ screen: the stack of a clip or a layer in the inspector, and the browser that lists every
// effect to drag onto one. The backend owns the kinds, the settings with their ranges and defaults, the order and
// the limit; this draws what its catalogue says and forwards the user's intent.

import { useState } from "react";
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
import type { MessageKey } from "../i18n";
import type { VjEffect, VjEffectKind, VjParamValue } from "../project/types";
import type { VjEffectSpec, VjParamSpec } from "./api";
import { ColourField, Scrub, Segments } from "./controls";
import { categoryKey, choiceKey, effectKey, genericParamKey, paramKey, specOf, useEffectCatalog } from "./effects";
import { edit, foldEffect, preview, useVj } from "./store";

/** What an effect dragged from the browser carries: the name of its kind. */
export const DRAG_EFFECT = "application/x-flux-vj-effect";
/** What an effect row dragged within a stack carries: its id. */
const DRAG_ROW = "application/x-flux-vj-fx";

/** What an effect is added to. */
export type EffectTarget = { clip: string } | { layer: string };

/** Whether a drag carries an effect from the browser. */
export const carriesEffect = (event: DragEvent) => event.dataTransfer.types.includes(DRAG_EFFECT);

/** Adds the effect a drop carries to `target`; whether it carried one. */
export function dropEffect(event: DragEvent, target: EffectTarget): boolean {
  const kind = event.dataTransfer.getData(DRAG_EFFECT);
  if (!kind) return false;
  event.preventDefault();
  event.stopPropagation();
  void edit({ op: "addEffect", target, kind });
  return true;
}

/** The count of effects drawn on a cell or a layer header. */
export function EffectBadge({ count }: { count: number }) {
  const t = useT();
  if (count === 0) return null;
  return <span className="vj-fxb" title={t("vj.effect.badge", { n: count })}>FX {count}</span>;
}

/** A length of a cycle in beats, as a musician writes it: 1/4, 1/2, 1, 2... */
const beatLabel = (beats: number) => (beats < 1 ? `1/${Math.round(1 / beats)}` : String(beats));

type Changed = Record<string, VjParamValue>;

/** The settings of one effect, each drawn as its catalogue entry says. */
function Settings({ id, spec, kind }: { id: string; spec: VjEffectSpec; kind: VjEffectKind }) {
  const t = useT();
  const withValues = (changed: Changed) => ({ ...kind, ...changed }) as VjEffectKind;
  const move = (changed: Changed) => preview({ op: "effectParams", effect: id, kind: withValues(changed) });
  const keep = (changed: Changed) => void edit({ op: "effectParams", effect: id, kind: withValues(changed) });
  const label = (key: string) => t(paramKey(spec.name, key)) ?? t(genericParamKey(key));

  const scrub = (param: Extract<VjParamSpec, { type: "number" }>, tag: string) => {
    const percent = param.unit === "%";
    const decimals = percent || param.whole ? 0 : param.step >= 0.1 ? 1 : 2;
    return (
      <Scrub key={param.key} label={tag} name={label(param.key)} value={kind[param.key] as number} shown={percent ? 100 : 1}
        reset={param.default} unit={param.unit} decimals={decimals} perPixel={(param.max - param.min) / 160}
        step={param.whole ? 1 : param.step} onMove={(v) => move({ [param.key]: v })} onSet={(v) => keep({ [param.key]: v })} />
    );
  };
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
        <div className="vj-f" key={param.key}><span>{t(paramKey(spec.name, param.key + next.key)) ?? t(genericParamKey(param.key + next.key))}</span>
          <div className="vj-scrubs">{scrub(param, first)}{scrub(next, second)}</div>
        </div>,
      );
      at++;
      continue;
    }
    rows.push(<div className="vj-f" key={param.key}><span>{label(param.key)}</span>{control(param)}</div>);
  }
  return <div className="vj-fx-p">{rows}</div>;
}

/** One effect of a stack: its name, fold, bypass, copy, reset, order and removal, and its settings. */
function EffectRow({ effect, index, stack, full }: { effect: VjEffect; index: number; stack: VjEffect[]; full: boolean }) {
  const t = useT();
  const catalog = useEffectCatalog((state) => state.catalog);
  const folded = useVj((vj) => vj.folded[effect.id] === true);
  const [edge, setEdge] = useState<"before" | "after" | null>(null);
  const { id, bypass, ...rest } = effect;
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
      {!folded && spec && <Settings id={id} spec={spec} kind={kind} />}
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
  return (
    <section className={`vj-fx${over ? " over" : ""}`} aria-label={t("vj.effects")}
      onDragOver={(event) => {
        if (!carriesEffect(event)) return;
        event.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(event) => {
        setOver(false);
        dropEffect(event, target);
      }}>
      <header className="vj-fx-head">
        <h3>{t("vj.effects")}<span className="vj-dim vj-fx-count">{effects.length}/{catalog?.max ?? "–"}</span></h3>
        <select className="vj-select wide vj-fx-add" value="" disabled={full || !catalog} aria-label={t("vj.effect.add")}
          title={full ? t("vj.effect.full", { n: max }) : undefined}
          onChange={(event) => { if (event.target.value) void edit({ op: "addEffect", target, kind: event.target.value }); }}>
          <option value="">{t("vj.effect.add")}</option>
          {catalog?.categories.map((category) => (
            <optgroup key={category} label={t(categoryKey(category))}>
              {catalog.effects.filter((spec) => spec.category === category).map((spec) => (
                <option key={spec.name} value={spec.name}>{t(effectKey(spec.name))}</option>
              ))}
            </optgroup>
          ))}
        </select>
      </header>
      {effects.length === 0 ? <p className="vj-dim vj-fx-empty">{t("vj.effect.empty")}</p> : (
        <ol className="vj-fx-list">
          {effects.map((effect, index) => <EffectRow key={effect.id} effect={effect} index={index} stack={effects} full={full} />)}
        </ol>
      )}
      {full && <p className="vj-dim vj-fx-empty">{t("vj.effect.full", { n: max })}</p>}
    </section>
  );
}

/** Every effect by category: drag one onto a clip, a layer or an inspector stack, or double click it to add it to `target`. */
export function EffectBrowser({ target }: { target: EffectTarget | null }) {
  const t = useT();
  const catalog = useEffectCatalog((state) => state.catalog);
  if (!catalog) return <p className="vj-empty">{t("vj.effect.loading")}</p>;
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
                  event.dataTransfer.effectAllowed = "copy";
                }}
                onDoubleClick={() => { if (target) void edit({ op: "addEffect", target, kind: spec.name }); }}
                onKeyDown={(event) => {
                  if ((event.key === "Enter" || event.key === " ") && target) void edit({ op: "addEffect", target, kind: spec.name });
                }}>
                <b>{t(effectKey(spec.name))}{spec.animated && <i className="vj-fxt" title={t("vj.effect.animated")}>↻</i>}</b>
                <span>{t(effectKey(spec.name, "hint"))}</span>
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
