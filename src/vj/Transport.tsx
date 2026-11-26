// The transport of a clip in the Inspector: its timeline strip with the region, the playhead and the cues, its
// direction, speed and tempo, and its eight cues. The backend owns where the playhead is and how it moves; this draws
// what the play state says and forwards what the hand does.

import { useRef, useState } from "react";
import type { CSSProperties, PointerEvent, ReactElement } from "react";
import { PauseIcon, PlayIcon } from "../components/icons";
import { useT } from "../i18n";
import type { VjClip, VjDirection } from "../project/types";
import type { VideoStrip } from "../video/api";
import { Scrub, Segments } from "./controls";
import { cue, edit, launch, preview, scrub, useVj } from "./store";

const DIRECTIONS: VjDirection[] = ["reverse", "pause", "forward"];
/** The lengths of one pass the backend offers (`TEMPO_BEATS` in vj.rs), or free. */
const TEMPOS = ["free", "1", "2", "4", "8", "16", "32", "64", "128"] as const;
type TempoChoice = (typeof TEMPOS)[number];
const CUES = 8;
const percent = (fraction: number) => `${fraction * 100}%`;

/** The mirror of the play icon, for backwards. */
const ReverseIcon = () => (
  <svg className="icon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" style={{ transform: "scaleX(-1)" }}>
    <path d="M4.5 3 L13 8 L4.5 13 Z" fill="currentColor" />
  </svg>
);

const DIRECTION_ICON: Record<VjDirection, () => ReactElement> = { reverse: ReverseIcon, pause: PauseIcon, forward: PlayIcon };

/** The strip of the clip: its region with in and out handles, the cues and the playhead; a click or a drag on it moves the playhead. */
function Strip({ clip, layer, strip }: { clip: VjClip; layer: string; strip: VideoStrip | undefined }) {
  const t = useT();
  const bar = useRef<HTMLDivElement>(null);
  const playing = useVj((vj) => vj.play.playing.find((p) => p.clip === clip.id && !p.stopping));
  const [held, setHeld] = useState<{ which: "start" | "end"; start: number; end: number } | null>(null);
  const [scrubbing, setScrubbing] = useState(false);
  const region = held ?? clip.region ?? { start: 0, end: 1 };
  const cues = clip.cues ?? [];
  const at = (event: { clientX: number }) => {
    const box = bar.current?.getBoundingClientRect();
    return box && box.width > 0 ? Math.min(1, Math.max(0, (event.clientX - box.left) / box.width)) : 0;
  };
  const moveTo = (position: number) => {
    // A clip that is not playing is launched where the hand lands.
    if (playing) scrub(layer, position);
    else void launch(clip.id, layer).then(() => scrub(layer, position));
  };
  const handle = (which: "start" | "end") => ({
    onPointerDown: (event: PointerEvent<HTMLElement>) => {
      event.stopPropagation();
      event.currentTarget.setPointerCapture(event.pointerId);
      setHeld({ which, ...region });
    },
    onPointerMove: (event: PointerEvent<HTMLElement>) => {
      if (!held || held.which !== which) return;
      const v = at(event);
      // The ends cannot cross; the backend keeps the least span between them.
      const next = which === "start" ? { start: Math.min(v, held.end), end: held.end } : { start: held.start, end: Math.max(v, held.start) };
      setHeld({ which, ...next });
      preview({ op: "clipRegion", clip: clip.id, ...next });
    },
    onPointerUp: () => {
      if (!held) return;
      const { start, end } = held;
      setHeld(null);
      void edit({ op: "clipRegion", clip: clip.id, start, end });
    },
    onPointerCancel: () => setHeld(null),
  });
  const background: CSSProperties = strip ? { backgroundImage: `url(${strip.url})` } : {};
  return (
    <div
      ref={bar}
      className={`vj-strip${scrubbing ? " scrubbing" : ""}`}
      style={background}
      role="slider"
      aria-label={t("vj.transport.bar")}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round((playing?.position ?? 0) * 100)}
      title={t("vj.transport.bar")}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        setScrubbing(true);
        moveTo(at(event));
      }}
      onPointerMove={(event) => { if (scrubbing) moveTo(at(event)); }}
      onPointerUp={() => setScrubbing(false)}
      onPointerCancel={() => setScrubbing(false)}
    >
      <i className="vj-outside left" style={{ width: percent(region.start) }} />
      <i className="vj-outside right" style={{ width: percent(1 - region.end) }} />
      {cues.map((position, index) => position != null && (
        <b key={index} className="vj-cuemark" style={{ left: percent(position) }}>{index + 1}</b>
      ))}
      {playing?.position != null && <u className="vj-head" style={{ left: percent(playing.position) }} />}
      <span className="vj-handle start" style={{ left: percent(region.start) }} role="separator" aria-orientation="vertical"
        aria-label={t("vj.transport.in")} title={t("vj.transport.in")} {...handle("start")} />
      <span className="vj-handle end" style={{ left: percent(region.end) }} role="separator" aria-orientation="vertical"
        aria-label={t("vj.transport.out")} title={t("vj.transport.out")} {...handle("end")} />
    </div>
  );
}

/** The transport of a clip. A generator and a still image have no file to run over: their duration is the length of it. */
export function Transport({ clip, layer, strip, still = false }: { clip: VjClip; layer: string; strip: VideoStrip | undefined; still?: boolean }) {
  const t = useT();
  const timed = clip.generator !== undefined || still;
  // Primitives, so the play state polled twenty times a second draws this only when the pass length changes.
  const passSeconds = useVj((vj) => vj.play.playing.find((p) => p.clip === clip.id && !p.stopping)?.passSeconds ?? null);
  const speed = clip.speed ?? 1;
  const direction = clip.direction ?? "forward";
  const beats = clip.tempo?.kind === "beats" ? clip.tempo.beats : null;
  const tempo: TempoChoice = beats === null ? "free" : (String(beats) as TempoChoice);
  const cues = clip.cues ?? [];
  const setCue = (index: number) => {
    // At the playhead; a clip that does not play puts it at the start of its region.
    const playing = useVj.getState().play.playing.find((p) => p.clip === clip.id && !p.stopping);
    const position = playing?.position ?? clip.region?.start ?? 0;
    void edit({ op: "clipCue", clip: clip.id, index, position });
  };
  return (
    <div className="vj-transport">
      <div className="vj-tlegend">{t("vj.transport.title")}</div>
      <Strip clip={clip} layer={layer} strip={strip} />
      {timed && (
        <div className="vj-f" title={t("vj.transport.durationHint")}><span>{t("vj.transport.duration")}</span>
          <div className="vj-scrubs one">
            <Scrub label="⏱" name={t("vj.transport.duration")} value={clip.duration ?? 10} shown={1} reset={10} unit="s" decimals={1} perPixel={0.1} step={1}
              onMove={(value) => preview({ op: "clipDuration", clip: clip.id, value })}
              onSet={(value) => void edit({ op: "clipDuration", clip: clip.id, value })} />
          </div>
        </div>
      )}
      <div className="vj-f"><span>{t("vj.transport.direction")}</span>
        <Segments options={DIRECTIONS} value={direction} title={(o) => t(`vj.transport.direction.${o}`)}
          label={(o) => { const Glyph = DIRECTION_ICON[o]; return <Glyph />; }}
          onPick={(next) => void edit({ op: "clipDirection", clip: clip.id, direction: next })} />
      </div>
      <div className={`vj-f${beats !== null ? " off" : ""}`} title={beats !== null ? t("vj.transport.speedOff") : undefined}>
        <span>{t("vj.transport.speed")}</span>
        <div className="vj-speed">
          <Scrub label="×" name={t("vj.transport.speed")} value={speed} shown={1} reset={1} unit="×" decimals={2} perPixel={0.02} step={0.05}
            onMove={(value) => preview({ op: "clipSpeed", clip: clip.id, value })}
            onSet={(value) => void edit({ op: "clipSpeed", clip: clip.id, value })} />
          <button type="button" className="vj-reset" disabled={speed === 1} title={t("vj.transport.speedReset")} aria-label={t("vj.transport.speedReset")}
            onClick={() => void edit({ op: "clipSpeed", clip: clip.id, value: 1 })}>1×</button>
        </div>
      </div>
      <div className="vj-f"><span>{t("vj.transport.tempo")}</span>
        <div className="vj-tempo">
          <Segments options={[...TEMPOS]} value={tempo}
            label={(o) => (o === "free" ? t("vj.transport.free") : o)}
            title={(o) => (o === "free" ? t("vj.transport.free") : t("vj.transport.beats", { n: o }))}
            onPick={(next) => void edit({ op: "clipTempo", clip: clip.id, tempo: next === "free" ? { kind: "free" } : { kind: "beats", beats: Number(next) } })} />
          <span className="vj-pass vj-mono">
            {beats === null ? "" : passSeconds != null ? t("vj.transport.pass", { seconds: passSeconds.toFixed(2) }) : t("vj.transport.passIdle")}
          </span>
        </div>
      </div>
      <div className="vj-f"><span>{t("vj.transport.cues")}</span>
        <div className="vj-cues" role="group" aria-label={t("vj.transport.cues")}>
          {Array.from({ length: CUES }, (_, index) => {
            const set = cues[index] != null;
            const n = index + 1;
            return (
              <span key={index} className={`vj-cue${set ? " set" : ""}`}>
                <button type="button"
                  title={t(set ? "vj.transport.cue.jump" : "vj.transport.cue.set", { n })}
                  aria-label={t(set ? "vj.transport.cue.jump" : "vj.transport.cue.set", { n })}
                  onClick={(event) => {
                    if (!set) setCue(index);
                    else if (event.altKey) void edit({ op: "clipCue", clip: clip.id, index, position: null });
                    else void cue(clip.id, index);
                  }}>{set ? n : <>{n}<small>{t("vj.transport.cue.empty")}</small></>}</button>
                {set && (
                  <button type="button" className="vj-cue-x" title={t("vj.transport.cue.clear", { n })} aria-label={t("vj.transport.cue.clear", { n })}
                    onClick={() => void edit({ op: "clipCue", clip: clip.id, index, position: null })}>×</button>
                )}
              </span>
            );
          })}
        </div>
      </div>
    </div>
  );
}
