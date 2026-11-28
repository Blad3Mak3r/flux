// The header of a track in the left column: icon, name, mute, level and routing.

import { useAudioOutputName } from "../audio/routing";
import { usePreferences } from "../preferences/store";
import { setCollapsed } from "../state/ui";
import { memo } from "react";
import type { PointerEvent } from "react";
import { useT } from "../i18n";
import type { Track } from "../project/types";
import { muteTrack, nameTrack, setGain } from "../state/actions";
import { menuAtPointer, rename, select, useUi } from "../state/ui";
import { AUDIO_LANE_H, NOTES_LANE_H } from "../timeline/layout";
import { traceStyle } from "../timeline/palette";
import { previewTrackGain } from "../transport/playback";
import { Fader } from "./Fader";
import { InlineName } from "./InlineName";
import { LevelMeter } from "./LevelMeter";
import { Marquee } from "./Marquee";
import { ChevronRightIcon, DmxIcon, MidiIcon, MuteIcon, NoteIcon, OscIcon, OutputIcon, VideoIcon, WaveIcon, VoiceIcon } from "./icons";
import { audioMenu, cueTrackMenu, notesTrackMenu, videoTrackMenu } from "./menus";

export function laneHeight(track: Track): number {
  return track.kind.type === "audio" || track.kind.type === "operator" || track.kind.type === "video" ? AUDIO_LANE_H : NOTES_LANE_H;
}

const KIND_ICONS = {
  audio: <WaveIcon />,
  operator: <VoiceIcon />,
  notes: <NoteIcon />,
  midi: <MidiIcon />,
  osc: <OscIcon />,
  dmx: <DmxIcon />,
  video: <VideoIcon />,
} as const;

export const TrackHeader = memo(function TrackHeader({
  track,
  dragging,
  onGrab,
}: {
  track: Track;
  dragging: boolean;
  /** Starts dragging the track to another place in the order. */
  onGrab: (event: PointerEvent<HTMLElement>, id: string) => void;
}) {
  const t = useT();
  const audio = track.kind.type === "audio" || track.kind.type === "operator" || track.kind.type === "video";
  const outputName = useAudioOutputName();
  const output = usePreferences((p) => p.output.trackRoutes?.find((r) => r.trackId === track.id)?.output);
  const selected = useUi((ui) => ui.selection?.kind === "track" && ui.selection.id === track.id);
  const renaming = useUi((ui) => ui.renaming?.kind === "track" && ui.renaming.id === track.id);
  return (
    <div
      className="track-header"
      style={{ height: laneHeight(track), ...traceStyle(track.color) }}
      data-selected={selected}
      onClick={() => select({ kind: "track", id: track.id })}
      data-muted={track.muted}
      data-audio={audio}
      data-colored={track.color !== undefined}
      data-dragging={dragging}
      onContextMenu={(event) =>
        menuAtPointer(
          event,
          audio
            ? audioMenu(t, track)
            : track.kind.type === "notes"
              ? notesTrackMenu(t, track)
              : track.kind.type === "video"
                ? videoTrackMenu(t, track)
                : cueTrackMenu(t, track),
        )
      }
    >
      <div className="track-head-row" onPointerDown={(event) => onGrab(event, track.id)}>
        <span className="track-kind" title={t(`track.kind.${track.kind.type}`)}>
          {KIND_ICONS[track.kind.type]}
          <span className="visually-hidden">{t(`track.kind.${track.kind.type}`)}</span>
        </span>
        {renaming ? (
          <InlineName
            value={track.name}
            label={t("track.rename", { name: track.name })}
            onCommit={(text) => nameTrack(track.id, text)}
            onDone={() => rename(null)}
          />
        ) : (
          <Marquee
            className="track-name"
            title={track.name}
            onDoubleClick={() => rename({ kind: "track", id: track.id })}
          >
            {track.name}
          </Marquee>
        )}
        <button
          type="button"
          className="icon-button"
          aria-pressed={track.muted}
          aria-label={t("track.mute", { name: track.name })}
          title={t("track.mute", { name: track.name })}
          onClick={() => muteTrack(track.id)}
        >
          <MuteIcon on={track.muted} />
        </button>
      </div>
      <div className="track-detail-row">
        {audio ? (
          <button
            type="button"
            className="track-output"
            aria-label={`${t("routing.trackOutput")}: ${outputName(output)}`}
            title={`${t("routing.trackOutput")}: ${outputName(output)}`}
            onClick={() => { select({ kind: "track", id: track.id }); setCollapsed(false); }}
          >
            <OutputIcon />
            <span className="track-output-name">{outputName(output)}</span>
            <ChevronRightIcon />
          </button>
        ) : (
          <span className="track-kind-label">{t(`track.kind.${track.kind.type}`)}</span>
        )}
        {track.muted && <span className="track-muted-tag">{t("track.muted")}</span>}
      </div>
      {audio && (
        <>
          <Fader
            name={track.name}
            valueDb={track.gainDb ?? 0}
            onPreview={(db) => previewTrackGain(track.id, db)}
            onCommit={(db) => setGain(track.id, db)}
          />
          <LevelMeter trackId={track.id} name={track.name} />
        </>
      )}
    </div>
  );
});
