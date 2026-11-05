// Pure edits of a Project. Every function returns a new project and never mutates its input, so
// the undo history can keep references to older ones.

import type {
  AudioAsset,
  AudioClip,
  FrameRate,
  OutputConfig,
  Project,
  Note,
  Track,
} from "../project/types";

export function emptyProject(name: string, frameRate: FrameRate): Project {
  const now = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
  return {
    formatVersion: 3,
    name,
    meta: { id: crypto.randomUUID(), createdAt: now, modifiedAt: now, createdWith: "", savedWith: "" },
    timecode: { frameRate, start: 0 },
    assets: [],
    tracks: [],
    output: { audio: null, ltc: [], mtc: [] },
  };
}

const newId = () => crypto.randomUUID();

/**
 * Applies `change` to every track. A track it returns as it got it keeps its identity, and so does
 * the project when none changed: what is drawn from a track skips the render when it is not hers.
 */
function mapTracks(project: Project, change: (track: Track) => Track): Project {
  const tracks = project.tracks.map(change);
  return tracks.every((track, index) => track === project.tracks[index]) ? project : { ...project, tracks };
}

export type Found =
  | { kind: "clip"; track: Track; clip: AudioClip }
  | { kind: "note"; track: Track; note: Note };

export function findItem(project: Project, id: string): Found | null {
  for (const track of project.tracks) {
    if (track.kind.type === "audio") {
      const clip = track.kind.clips.find((c) => c.id === id);
      if (clip) return { kind: "clip", track, clip };
    } else {
      const note = track.kind.notes.find((n) => n.id === id);
      if (note) return { kind: "note", track, note };
    }
  }
  return null;
}

export function updateClip(project: Project, id: string, patch: Partial<AudioClip>): Project {
  return mapTracks(project, (track) =>
    track.kind.type === "audio" && track.kind.clips.some((c) => c.id === id)
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

export function updateNote(project: Project, id: string, patch: Partial<Note>): Project {
  return mapTracks(project, (track) =>
    track.kind.type === "notes" && track.kind.notes.some((n) => n.id === id)
      ? {
          ...track,
          kind: {
            type: "notes",
            notes: track.kind.notes.map((n) => (n.id === id ? { ...n, ...patch } : n)),
          },
        }
      : track,
  );
}

/** Sets how long a note lasts; `null` (or no time) returns to "until the next note", stored as no value. */
export function setNoteLength(project: Project, id: string, length: number | null): Project {
  const found = findItem(project, id);
  if (found?.kind !== "note") return project;
  const { length: _previous, ...rest } = found.note;
  const next = length !== null && length >= 1 ? { ...rest, length: Math.round(length) } : rest;
  return replaceNote(project, id, next);
}

/** Puts `note` in place of the note with `id`, key for key (a patch cannot take a key away). */
function replaceNote(project: Project, id: string, note: Note): Project {
  return mapTracks(project, (track) =>
    track.kind.type === "notes" && track.kind.notes.some((n) => n.id === id)
      ? { ...track, kind: { type: "notes", notes: track.kind.notes.map((n) => (n.id === id ? note : n)) } }
      : track,
  );
}

/** Moves a clip or a note to `position` frames from the timeline origin (never before it). */
export function moveItem(project: Project, id: string, position: number): Project {
  const at = Math.max(0, Math.round(position));
  const found = findItem(project, id);
  if (!found) return project;
  return found.kind === "clip"
    ? updateClip(project, id, { start: at })
    : updateNote(project, id, { at });
}

/**
 * Removes a clip or a note. An audio track is a song: when its last clip goes the track goes with
 * it, and so does an audio file no other clip uses. A notes track stays, empty, to take the next note.
 */
export function removeItem(project: Project, id: string): Project {
  const removed = mapTracks(project, (track) => {
    if (track.kind.type === "audio") {
      const { clips } = track.kind;
      return clips.some((c) => c.id === id)
        ? { ...track, kind: { type: "audio", clips: clips.filter((c) => c.id !== id) } }
        : track;
    }
    const { notes } = track.kind;
    return notes.some((n) => n.id === id)
      ? { ...track, kind: { type: "notes", notes: notes.filter((n) => n.id !== id) } }
      : track;
  });
  return removed === project ? project : dropUnused(removed);
}

/** Drops audio tracks left with no clip and the audio files no clip uses. */
function dropUnused(project: Project): Project {
  const tracks = project.tracks.filter((track) => track.kind.type !== "audio" || track.kind.clips.length > 0);
  const used = new Set(tracks.flatMap((track) => (track.kind.type === "audio" ? track.kind.clips.map((c) => c.asset) : [])));
  const assets = project.assets.filter((asset) => used.has(asset.id));
  return tracks.length === project.tracks.length && assets.length === project.assets.length
    ? project
    : { ...project, tracks, assets };
}

/** Removes a whole track with its clips or notes, and the audio files nothing else uses. */
export function removeTrack(project: Project, trackId: string): Project {
  const tracks = project.tracks.filter((track) => track.id !== trackId);
  return tracks.length === project.tracks.length ? project : dropUnused({ ...project, tracks });
}

/** Moves a track to index `to` of the track order (clamped); the order is the order on screen and in the mix. */
export function moveTrack(project: Project, trackId: string, to: number): Project {
  const from = project.tracks.findIndex((track) => track.id === trackId);
  const index = Math.min(Math.max(to, 0), project.tracks.length - 1);
  if (from === -1 || from === index) return project;
  const tracks = [...project.tracks];
  tracks.splice(index, 0, ...tracks.splice(from, 1));
  return { ...project, tracks };
}

export function toggleMute(project: Project, trackId: string): Project {
  return mapTracks(project, (track) =>
    track.id === trackId ? { ...track, muted: !track.muted } : track,
  );
}

/** Sets an audio track's fader. 0 dB is stored as no value, as in a file that never had one. */
export function setTrackGain(project: Project, trackId: string, gainDb: number): Project {
  const target = project.tracks.find((track) => track.id === trackId);
  if (!target || target.kind.type !== "audio" || (target.gainDb ?? 0) === gainDb) return project;
  return mapTracks(project, (track) => {
    if (track.id !== trackId) return track;
    const { gainDb: _previous, ...rest } = track;
    return gainDb === 0 ? rest : { ...rest, gainDb };
  });
}

/** Sets the colour a track is drawn in; `null` returns to the default, stored as no value. */
export function setTrackColor(project: Project, trackId: string, color: string | null): Project {
  const target = project.tracks.find((track) => track.id === trackId);
  if (!target || (target.color ?? null) === color) return project;
  return mapTracks(project, (track) => {
    if (track.id !== trackId) return track;
    const { color: _previous, ...rest } = track;
    return color === null ? rest : { ...rest, color };
  });
}

/**
 * Adds an empty note at `at` (frames from the timeline origin). It goes on `trackId` when that is a
 * notes track, else on the first notes track, creating one named `trackName` when there is none.
 */
export function addNote(
  project: Project,
  at: number,
  trackName: string,
  trackId?: string,
): { project: Project; id: string } {
  const id = newId();
  const note: Note = { id, at: Math.max(0, Math.round(at)), text: "" };

  const isNotes = (track: Track) => track.kind.type === "notes";
  const target = project.tracks.findIndex((track) => track.id === trackId && isNotes(track));
  const index = target !== -1 ? target : project.tracks.findIndex(isNotes);
  if (index === -1) {
    const track: Track = { id: newId(), name: trackName, muted: false, kind: { type: "notes", notes: [note] } };
    return { project: { ...project, tracks: [...project.tracks, track] }, id };
  }
  const tracks = project.tracks.map((track, i) =>
    i === index && track.kind.type === "notes"
      ? { ...track, kind: { type: "notes" as const, notes: [...track.kind.notes, note] } }
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
