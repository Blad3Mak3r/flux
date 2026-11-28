// The faders of a layer header (master, audio, video, transition) and the time of the clip on air.

import { useState } from "react";
import type { CSSProperties, KeyboardEvent, PointerEvent } from "react";
import { PlayIcon } from "../components/icons";
import { useT } from "../i18n";
import type { VjTarget } from "../project/types";
import { useVj } from "./store";
import { mappable } from "./mapModel";
import { clock } from "./VideoThumb";

/** A vertical fader: its letter is the handle, its colour says what it moves. */
export function Fader({ letter, label, value, off, progress, modulated, map, onMove, onSet, onReset }: {
  letter: string;
  label: string;
  value: number;
  off?: boolean;
  /** The layer and field of this fader when it is modulated: a mark shows where the modulated value is. */
  modulated?: { layer: string; field: string };
  /** How far what the fader times is, 0 to 1, while it runs: the transition of the T fader. */
  progress?: number | null;
  /** What the fader is in the map mode. */
  map?: VjTarget;
  onMove: (value: number) => void;
  onSet: (value: number) => void;
  /** Right click: back to the default the backend holds. */
  onReset: () => void;
}) {
  const [held, setHeld] = useState<number | null>(null);
  const shown = held ?? value;
  const now = useVj((vj) => (modulated ? vj.mod.layers[modulated.layer]?.[modulated.field] : undefined));
  const t = useT();
  const at = (event: PointerEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return Math.min(1, Math.max(0, 1 - (event.clientY - box.top) / box.height));
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.key === "ArrowUp" ? 0.05 : event.key === "ArrowDown" ? -0.05 : 0;
    if (!step) return;
    event.preventDefault();
    event.stopPropagation();
    onSet(Math.min(1, Math.max(0, value + step)));
  };
  return (
    <div
      className={`vj-fader${off ? " off" : ""}${modulated ? " modulated" : ""}`}
      data-letter={letter}
      {...mappable(map)}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(shown * 100)}
      title={`${label} ${Math.round(shown * 100)}${modulated ? ` · ${t("vj.layer.modulated")}` : ""} · ${t("vj.fader.reset")}`}
      style={{ "--v": shown, ...(now === undefined ? {} : { "--mv": Math.min(1, Math.max(0, now)) }) } as CSSProperties}
      onKeyDown={onKeyDown}
      onContextMenu={(event) => {
        event.preventDefault();
        event.stopPropagation();
        setHeld(null);
        onReset();
      }}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        const v = at(event);
        setHeld(v);
        onMove(v);
      }}
      onPointerMove={(event) => {
        if (held === null) return;
        const v = at(event);
        setHeld(v);
        onMove(v);
      }}
      onPointerUp={(event) => {
        if (held === null) return;
        const v = at(event);
        setHeld(null);
        onSet(v);
      }}
      onPointerCancel={() => setHeld(null)}
    >
      <i />
      {progress != null && <u style={{ "--p": progress } as CSSProperties} />}
      {now !== undefined && <em aria-hidden="true" />}
      <b>{letter}</b>
    </div>
  );
}

/** The countdown and progress of what `layer` plays (its `clip` only, when given): the one part
 * that changes on every poll, so the cell or header around it does not draw again. */
export function PlayTime({ layer, clip, countdown }: { layer: string; clip?: string; countdown: "header" | "cell" }) {
  const shown = (vj: ReturnType<typeof useVj.getState>) =>
    vj.play.playing.find((p) => p.layer === layer && (clip === undefined || p.clip === clip));
  const progress = useVj((vj) => shown(vj)?.progress);
  const remaining = useVj((vj) => shown(vj)?.remaining);
  const left = remaining == null ? null : clock(remaining);
  return (
    <>
      {countdown === "header" && left !== null && <span className="vj-time">-{left}</span>}
      {countdown === "cell" && <span className="vj-air"><PlayIcon />{left ?? ""}</span>}
      {progress != null && <span className="vj-bar"><i style={{ width: `${progress * 100}%` }} /></span>}
    </>
  );
}
