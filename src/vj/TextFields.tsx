// The inspector's controls of a text block generator: its text, font, colour, size, position and spacing, and which of its lines shows.

import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { useT } from "../i18n";
import type { MessageKey } from "../i18n";
import type { VjClip, VjGenerator, VjTextAlign, VjTextLines } from "../project/types";
import { ColourField, Segments } from "./controls";
import { useFonts } from "./fonts";
import { GeneratorModPanel, GeneratorScrub } from "./Modulation";
import { edit, preview } from "./store";

type Text = Extract<VjGenerator, { kind: "text" }>;

const ALIGNS: VjTextAlign[] = ["left", "center", "right"];
const LINES: VjTextLines[] = ["all", "one"];
/** How many beats the line takes to step to the next; 0 is by hand. */
const ADVANCES = ["0", "1", "2", "4", "8", "16"];
/** The faces named in the app's words; any other face keeps the name its font gives it. */
const STYLES: Record<string, MessageKey> = {
  Regular: "vj.gen.style.regular",
  Bold: "vj.gen.style.bold",
  Italic: "vj.gen.style.italic",
  "Bold Italic": "vj.gen.style.boldItalic",
};

/** The text, typed in place: it goes to the backend when the box loses focus or with Ctrl+Enter, not on each key. */
function TextBox({ value, label, onCommit }: { value: string; label: string; onCommit: (text: string) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <textarea className="vj-input vj-textbox" rows={4} maxLength={4000} spellCheck={false} aria-label={label} value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => { if (draft !== value) onCommit(draft); }}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) event.currentTarget.blur();
        if (event.key === "Escape") { setDraft(value); event.currentTarget.blur(); }
      }} />
  );
}

type NumberKey = "size" | "opacity" | "positionX" | "positionY" | "rotation" | "tracking" | "leading";
type NumberProps = { label: string; name: string; shown: number; reset: number; unit: string; decimals?: number };

export function TextFields({ clip, generator, modulation }: { clip: string; generator: Text; modulation: VjClip["modulation"] }) {
  const t = useT();
  const fonts = useFonts();
  const g = {
    text: generator.text ?? "Text",
    font: generator.font ?? "Inter",
    style: generator.style ?? "Bold",
    size: generator.size ?? 0.12,
    colour: generator.colour ?? "#ffffff",
    opacity: generator.opacity ?? 1,
    align: generator.align ?? "center",
    positionX: generator.positionX ?? 0.5,
    positionY: generator.positionY ?? 0.5,
    rotation: generator.rotation ?? 0,
    tracking: generator.tracking ?? 0,
    leading: generator.leading ?? 0,
    lines: generator.lines ?? "all",
    line: generator.line ?? 0,
    advance: generator.advance ?? 0,
  };
  const move = (changed: Partial<Text>) => preview({ op: "clipGenerator", clip, generator: { ...generator, ...changed } });
  const keep = (changed: Partial<Text>) => void edit({ op: "clipGenerator", clip, generator: { ...generator, ...changed } });
  const number = (key: NumberKey, props: NumberProps) => (
    <GeneratorScrub {...props} clip={clip} modulation={modulation} field={key} value={g[key]} onMove={(v) => move({ [key]: v })} onSet={(v) => keep({ [key]: v })} />
  );
  const panel = (key: NumberKey, props: NumberProps) => (
    <GeneratorModPanel clip={clip} modulation={modulation} field={key} shown={props.shown} unit={props.unit} decimals={props.decimals} />
  );
  const row = (name: string, control: ReactNode, hint?: string) => (
    <div className="vj-f" title={hint}><span>{name}</span>{control}</div>
  );
  const one = (key: NumberKey, name: string, props: NumberProps) => (
    <>
      {row(name, <div className="vj-scrubs one">{number(key, props)}</div>)}
      {panel(key, props)}
    </>
  );

  const family = fonts.find((f) => f.name === g.font);
  const families = family ? fonts : [{ name: g.font, styles: [g.style] }, ...fonts];
  const styles = family?.styles.includes(g.style) || !family ? (family?.styles ?? [g.style]) : [g.style, ...family.styles];
  const styleName = (name: string) => (STYLES[name] ? t(STYLES[name]) : name);
  const lineCount = g.text.split("\n").length;

  return (
    <>
      {row(t("vj.gen.text"), <TextBox value={g.text} label={t("vj.gen.text")} onCommit={(text) => keep({ text })} />, t("vj.gen.textHint"))}
      {row(t("vj.gen.font"),
        <select className="vj-select wide" value={g.font} aria-label={t("vj.gen.font")}
          onChange={(event) => {
            const next = fonts.find((f) => f.name === event.target.value);
            // The same face if the family has it, else its regular one.
            const style = next?.styles.includes(g.style) ? g.style : next?.styles.includes("Regular") ? "Regular" : (next?.styles[0] ?? g.style);
            keep({ font: event.target.value, style });
          }}>
          {families.map((f) => <option key={f.name} value={f.name}>{f.name}</option>)}
        </select>)}
      {row(t("vj.gen.style"),
        <select className="vj-select wide" value={g.style} aria-label={t("vj.gen.style")} onChange={(event) => keep({ style: event.target.value })}>
          {styles.map((s) => <option key={s} value={s}>{styleName(s)}</option>)}
        </select>)}
      {row(t("vj.gen.colour"), <ColourField value={g.colour} label={t("vj.gen.colour")} onMove={(colour) => move({ colour })} onSet={(colour) => keep({ colour })} />)}
      {one("size", t("vj.gen.size"), { label: "S", name: t("vj.gen.size"), shown: 100, reset: 0.12, unit: "%", decimals: 1 })}
      {one("opacity", t("vj.gen.opacity"), { label: "O", name: t("vj.gen.opacity"), shown: 100, reset: 1, unit: "%" })}
      {row(t("vj.gen.position"),
        <div className="vj-scrubs">
          {number("positionX", { label: "X", name: t("vj.gen.positionX"), shown: 100, reset: 0.5, unit: "%" })}
          {number("positionY", { label: "Y", name: t("vj.gen.positionY"), shown: 100, reset: 0.5, unit: "%" })}
        </div>)}
      {panel("positionX", { label: "X", name: t("vj.gen.positionX"), shown: 100, reset: 0.5, unit: "%" })}
      {panel("positionY", { label: "Y", name: t("vj.gen.positionY"), shown: 100, reset: 0.5, unit: "%" })}
      {one("rotation", t("vj.gen.rotation"), { label: "R", name: t("vj.gen.rotation"), shown: 1, reset: 0, unit: "°" })}
      {one("tracking", t("vj.gen.tracking"), { label: "X", name: t("vj.gen.tracking"), shown: 100, reset: 0, unit: "%" })}
      {one("leading", t("vj.gen.leading"), { label: "Y", name: t("vj.gen.leading"), shown: 100, reset: 0, unit: "%" })}
      {row(t("vj.gen.align"), <Segments options={ALIGNS} value={g.align} label={(o) => t(`vj.gen.align.${o}`)} onPick={(align) => keep({ align })} />)}
      {row(t("vj.gen.lines"), <Segments options={LINES} value={g.lines} label={(o) => t(`vj.gen.lines.${o}`)} onPick={(lines) => keep({ lines })} />)}
      {g.lines === "one" && (
        <>
          {row(t("vj.gen.line"),
            <div className="vj-segs vj-linestep">
              <button type="button" title={t("vj.gen.linePrev")} aria-label={t("vj.gen.linePrev")} onClick={() => void edit({ op: "textLine", clip, step: -1 })}>‹</button>
              <span className="vj-mono" aria-live="polite">{Math.min(g.line, lineCount - 1) + 1} / {lineCount}</span>
              <button type="button" title={t("vj.gen.lineNext")} aria-label={t("vj.gen.lineNext")} onClick={() => void edit({ op: "textLine", clip, step: 1 })}>›</button>
            </div>, t("vj.gen.lineHint"))}
          {row(t("vj.gen.advance"),
            <div className="vj-advance"><Segments options={ADVANCES} value={String(g.advance)} label={(o) => (o === "0" ? t("vj.gen.advance.off") : o)}
              title={(o) => (o === "0" ? t("vj.gen.advance.off") : t("vj.gen.advance.beats", { n: o }))}
              onPick={(o) => keep({ advance: Number(o) })} /></div>, t("vj.gen.advanceHint"))}
        </>
      )}
    </>
  );
}
