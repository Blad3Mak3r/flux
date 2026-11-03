// Pure edits of a Project. Every function returns a new project and never mutates its input, so
// the undo history can keep references to older ones.

import type {
  AudioAsset,
  AudioClip,
  FrameRate,
  OutputConfig,
  Project,
  Track,
  Trigger,
} from "../project/types";

export function emptyProject(name: string, frameRate: FrameRate): Project {
  return {
    formatVersion: 1,
    name,
    timecode: { frameRate, start: 0 },
    assets: [],
    tracks: [],
    output: { audio: null, ltc: null, mtc: null },
  };
}

const newId = () => crypto.randomUUID();

function mapTracks(project: Project, change: (track: Track) => Track): Project {
  return { ...project, tracks: project.tracks.map(change) };
}

export type Found =
  | { kind: "clip"; track: Track; clip: AudioClip }
  | { kind: "trigger"; track: Track; trigger: Trigger };

export function findItem(project: Project, id: string): Found | null {
  for (const track of project.tracks) {
    if (track.kind.type === "audio") {
      const clip = track.kind.clips.find((c) => c.id === id);
      if (clip) return { kind: "clip", track, clip };
    } else {
      const trigger = track.kind.triggers.find((t) => t.id === id);
      if (trigger) return { kind: "trigger", track, trigger };
    }
  }
  return null;
}

export function updateClip(project: Project, id: string, patch: Partial<AudioClip>): Project {
  return mapTracks(project, (track) =>
    track.kind.type === "audio"
      ? {
          ...track,
          kind: {
            type: "audio",
            clips: track.kind.clips.map((c) => (c.id === id ? { ...c, ...patch } : c)),
          },
        }
      : track,
  );
}

export function updateTrigger(project: Project, id: string, patch: Partial<Trigger>): Project {
  return mapTracks(project, (track) =>
    track.kind.type === "events"
      ? {
          ...track,
          kind: {
            type: "events",
            triggers: track.kind.triggers.map((t) => (t.id === id ? { ...t, ...patch } : t)),
          },
        }
      : track,
  );
}

/** Moves a clip or a cue to `position` frames from the timeline origin (never before it). */
export function moveItem(project: Project, id: string, position: number): Project {
  const at = Math.max(0, Math.round(position));
  const found = findItem(project, id);
  if (!found) return project;
  return found.kind === "clip"
    ? updateClip(project, id, { start: at })
    : updateTrigger(project, id, { at });
}

export function removeItem(project: Project, id: string): Project {
  return mapTracks(project, (track) =>
    track.kind.type === "audio"
      ? { ...track, kind: { type: "audio", clips: track.kind.clips.filter((c) => c.id !== id) } }
      : { ...track, kind: { type: "events", triggers: track.kind.triggers.filter((t) => t.id !== id) } },
  );
}

export function toggleMute(project: Project, trackId: string): Project {
  return mapTracks(project, (track) =>
    track.id === trackId ? { ...track, muted: !track.muted } : track,
  );
}

/** Adds a marker at `at` on the first events track, creating one when there is none. */
export function addMarker(
  project: Project,
  at: number,
  label: (count: number) => string,
  trackName: string,
): { project: Project; id: string } {
  const id = newId();
  const count =
    project.tracks.reduce(
      (sum, track) => sum + (track.kind.type === "events" ? track.kind.triggers.length : 0),
      0,
    ) + 1;
  const trigger: Trigger = { id, at: Math.max(0, at), label: label(count), action: { type: "marker" } };

  const existing = project.tracks.findIndex((track) => track.kind.type === "events");
  if (existing === -1) {
    const track: Track = {
      id: newId(),
      name: trackName,
      muted: false,
      kind: { type: "events", triggers: [trigger] },
    };
    return { project: { ...project, tracks: [...project.tracks, track] }, id };
  }
  const tracks = project.tracks.map((track, index) =>
    index === existing && track.kind.type === "events"
      ? { ...track, kind: { type: "events" as const, triggers: [...track.kind.triggers, trigger] } }
      : track,
  );
  return { project: { ...project, tracks }, id };
}

/** Adds an imported audio file on a new track of its own, starting at `at`. */
export function addAudioClip(
  project: Project,
  asset: AudioAsset,
  at: number,
): { project: Project; id: string } {
  const id = newId();
  const clip: AudioClip = { id, asset: asset.id, start: Math.max(0, at), gainDb: 0 };
  const track: Track = {
    id: newId(),
    name: asset.name,
    muted: false,
    kind: { type: "audio", clips: [clip] },
  };
  return {
    project: { ...project, assets: [...project.assets, asset], tracks: [...project.tracks, track] },
    id,
  };
}

export function setName(project: Project, name: string): Project {
  return { ...project, name };
}

export function setFrameRate(project: Project, frameRate: FrameRate): Project {
  return { ...project, timecode: { ...project.timecode, frameRate } };
}

export function setStart(project: Project, start: number): Project {
  return { ...project, timecode: { ...project.timecode, start } };
}

export function setOutput(project: Project, output: OutputConfig): Project {
  return { ...project, output };
}
