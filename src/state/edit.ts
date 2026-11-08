// Pure edits of a Project. Every function returns a new project and never mutates its input, so
// the undo history can keep references to older ones. The edits of tracks, clips and notes take the
// id of the timeline they are in; which timelines exist, which is active and its timecode are the
// backend's business (see `state/timelines.ts`).

import type {
  AudioAsset,
  AudioClip,
  FrameRate,
  MidiEvent,
  OscEvent,
  Project,
  Note,
  Tempo,
  Timeline,
  Track,
} from "../project/types";
import { DEFAULT_TEMPO, MAX_BEATS_PER_BAR, MAX_BPM, MIN_BPM } from "../project/types.ts";

export function emptyProject(name: string, frameRate: FrameRate): Project {
  const now = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
  return {
    formatVersion: 12,
    autosave: { enabled: true, intervalSeconds: 30 },
    name,
    meta: { id: crypto.randomUUID(), createdAt: now, modifiedAt: now, createdWith: "", savedWith: "" },
    assets: [],
    output: { ltc: [], mtc: [] },
    timelines: [
      { id: crypto.randomUUID(), name, timecode: { frameRate, start: 0 }, tempo: DEFAULT_TEMPO, tracks: [] },
    ],
  };
}

const newId = () => crypto.randomUUID();

/**
 * Applies `change` to the timeline `timeline`. One it returns as it got it keeps its identity, and
 * so does the project: what is drawn from a timeline skips the render when it is not hers.
 */
function inTimeline(project: Project, timeline: string, change: (timeline: Timeline) => Timeline): Project {
  const timelines = project.timelines.map((t) => (t.id === timeline ? change(t) : t));
  return timelines.every((t, index) => t === project.timelines[index]) ? project : { ...project, timelines };
}

/**
 * Applies `change` to every track of the timeline. A track it returns as it got it keeps its
 * identity, and so does the project when none changed.
 */
function mapTracks(project: Project, timeline: string, change: (track: Track) => Track): Project {
  return inTimeline(project, timeline, (t) => {
    const tracks = t.tracks.map(change);
    return tracks.every((track, index) => track === t.tracks[index]) ? t : { ...t, tracks };
  });
}

export type Found =
  | { kind: "clip"; track: Track; clip: AudioClip }
  | { kind: "note"; track: Track; note: Note }
  | { kind: "midiEvent"; track: Track; event: MidiEvent }
  | { kind: "oscEvent"; track: Track; event: OscEvent };

/** Finds a clip or a note by its id, which is unique in the whole project. */
export function findItem(project: Project, id: string): Found | null {
  for (const track of project.timelines.flatMap((t) => t.tracks)) {
    if (track.kind.type === "audio") {
      const clip = track.kind.clips.find((c) => c.id === id);
      if (clip) return { kind: "clip", track, clip };
    } else if (track.kind.type === "notes") {
      const note = track.kind.notes.find((n) => n.id === id);
      if (note) return { kind: "note", track, note };
    } else if (track.kind.type === "midi") {
      const event = track.kind.events.find((e) => e.id === id);
      if (event) return { kind: "midiEvent", track, event };
    } else if (track.kind.type === "osc") {
      const event = track.kind.events.find((e) => e.id === id);
      if (event) return { kind: "oscEvent", track, event };
    }
  }
  return null;
}

export function updateClip(project: Project, timeline: string, id: string, patch: Partial<AudioClip>): Project {
  return mapTracks(project, timeline, (track) =>
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

export function updateNote(project: Project, timeline: string, id: string, patch: Partial<Note>): Project {
  return mapTracks(project, timeline, (track) =>
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
export function setNoteLength(project: Project, timeline: string, id: string, length: number | null): Project {
  const found = findItem(project, id);
  if (found?.kind !== "note") return project;
  const { length: _previous, ...rest } = found.note;
  const next = length !== null && length >= 1 ? { ...rest, length: Math.round(length) } : rest;
  return replaceNote(project, timeline, id, next);
}

/** Puts `note` in place of the note with `id`, key for key (a patch cannot take a key away). */
function replaceNote(project: Project, timeline: string, id: string, note: Note): Project {
  return mapTracks(project, timeline, (track) =>
    track.kind.type === "notes" && track.kind.notes.some((n) => n.id === id)
      ? { ...track, kind: { type: "notes", notes: track.kind.notes.map((n) => (n.id === id ? note : n)) } }
      : track,
  );
}

/** Moves a clip or a note to `position` frames from the timeline origin (never before it). */
export function moveItem(project: Project, timeline: string, id: string, position: number): Project {
  const at = Math.max(0, Math.round(position));
  const found = findItem(project, id);
  if (!found) return project;
  if (found.kind === "clip") return updateClip(project, timeline, id, { start: at });
  return found.kind === "note" ? updateNote(project, timeline, id, { at }) : project;
}

/**
 * Removes a clip or a note. An audio track is a song: when its last clip goes the track goes with
 * it, and so does an audio file no other clip uses. A notes track stays, empty, to take the next note.
 */
export function removeItem(project: Project, timeline: string, id: string): Project {
  const removed = mapTracks(project, timeline, (track) => {
    if (track.kind.type === "audio") {
      const { clips } = track.kind;
      return clips.some((c) => c.id === id)
        ? { ...track, kind: { type: "audio", clips: clips.filter((c) => c.id !== id) } }
        : track;
    }
    if (track.kind.type !== "notes") return track;
    const { notes } = track.kind;
    return notes.some((n) => n.id === id)
      ? { ...track, kind: { type: "notes", notes: notes.filter((n) => n.id !== id) } }
      : track;
  });
  return removed === project ? project : dropUnused(removed, timeline);
}

/**
 * Drops the audio tracks of the timeline left with no clip, and the audio files no clip of any
 * timeline uses.
 */
function dropUnused(project: Project, timeline: string): Project {
  const pruned = inTimeline(project, timeline, (t) => {
    const tracks = t.tracks.filter((track) => track.kind.type !== "audio" || track.kind.clips.length > 0);
    return tracks.length === t.tracks.length ? t : { ...t, tracks };
  });
  const used = new Set(
    pruned.timelines.flatMap((t) =>
      t.tracks.flatMap((track) => (track.kind.type === "audio" ? track.kind.clips.map((c) => c.asset) : track.kind.type === "operator" ? track.kind.orders.flatMap((o) => o.generated ? [o.generated.asset] : []) : [])),
    ),
  );
  const assets = pruned.assets.filter((asset) => used.has(asset.id));
  return assets.length === pruned.assets.length ? pruned : { ...pruned, assets };
}

/** Removes a whole track with its clips or notes, and the audio files nothing else uses. */
export function removeTrack(project: Project, timeline: string, trackId: string): Project {
  const removed = inTimeline(project, timeline, (t) => {
    const tracks = t.tracks.filter((track) => track.id !== trackId);
    return tracks.length === t.tracks.length ? t : { ...t, tracks };
  });
  return removed === project ? project : dropUnused(removed, timeline);
}

/** Moves a track to index `to` of the track order (clamped); the order is the order on screen and in the mix. */
export function moveTrack(project: Project, timeline: string, trackId: string, to: number): Project {
  return inTimeline(project, timeline, (t) => {
    const from = t.tracks.findIndex((track) => track.id === trackId);
    const index = Math.min(Math.max(to, 0), t.tracks.length - 1);
    if (from === -1 || from === index) return t;
    const tracks = [...t.tracks];
    tracks.splice(index, 0, ...tracks.splice(from, 1));
    return { ...t, tracks };
  });
}

export function toggleMute(project: Project, timeline: string, trackId: string): Project {
  return mapTracks(project, timeline, (track) =>
    track.id === trackId ? { ...track, muted: !track.muted } : track,
  );
}

const trackOf = (project: Project, timeline: string, trackId: string) =>
  project.timelines.find((t) => t.id === timeline)?.tracks.find((track) => track.id === trackId);

/** Sets an audio track's fader. 0 dB is stored as no value, as in a file that never had one. */
export function setTrackGain(project: Project, timeline: string, trackId: string, gainDb: number): Project {
  const target = trackOf(project, timeline, trackId);
  if (!target || target.kind.type !== "audio" || (target.gainDb ?? 0) === gainDb) return project;
  return mapTracks(project, timeline, (track) => {
    if (track.id !== trackId) return track;
    const { gainDb: _previous, ...rest } = track;
    return gainDb === 0 ? rest : { ...rest, gainDb };
  });
}

/** Sets the colour a track is drawn in; `null` returns to the default, stored as no value. */
export function setTrackColor(project: Project, timeline: string, trackId: string, color: string | null): Project {
  const target = trackOf(project, timeline, trackId);
  if (!target || (target.color ?? null) === color) return project;
  return mapTracks(project, timeline, (track) => {
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
  timeline: string,
  at: number,
  trackName: string,
  trackId?: string,
): { project: Project; id: string } {
  const id = newId();
  const note: Note = { id, at: Math.max(0, Math.round(at)), text: "" };

  const isNotes = (track: Track) => track.kind.type === "notes";
  const next = inTimeline(project, timeline, (t) => {
    const target = t.tracks.findIndex((track) => track.id === trackId && isNotes(track));
    const index = target !== -1 ? target : t.tracks.findIndex(isNotes);
    if (index === -1) {
      const track: Track = { id: newId(), name: trackName, muted: false, kind: { type: "notes", notes: [note] } };
      return { ...t, tracks: [...t.tracks, track] };
    }
    const tracks = t.tracks.map((track, i) =>
      i === index && track.kind.type === "notes"
        ? { ...track, kind: { type: "notes" as const, notes: [...track.kind.notes, note] } }
        : track,
    );
    return { ...t, tracks };
  });
  return { project: next, id };
}

/** Adds an imported audio file on a new track of its own, starting at `at`. */
export function addAudioClip(
  project: Project,
  timeline: string,
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
  const next = inTimeline({ ...project, assets: [...project.assets, asset] }, timeline, (t) => ({
    ...t,
    tracks: [...t.tracks, track],
  }));
  return { project: next, id };
}

/** A tempo the backend accepts: a BPM in range, to two decimals, and a whole number of beats a bar. */
export function clampTempo(tempo: Tempo): Tempo {
  const bpm = Number.isFinite(tempo.bpm) ? Math.min(MAX_BPM, Math.max(MIN_BPM, tempo.bpm)) : DEFAULT_TEMPO.bpm;
  const beats = Number.isFinite(tempo.beatsPerBar) ? Math.round(tempo.beatsPerBar) : DEFAULT_TEMPO.beatsPerBar;
  return { ...tempo, bpm: Math.round(bpm * 100) / 100, beatsPerBar: Math.min(MAX_BEATS_PER_BAR, Math.max(1, beats)) };
}

/** Changes the tempo of a timeline. One that comes out the same keeps the project as it was. */
export function setTempo(project: Project, timeline: string, patch: Partial<Tempo>): Project {
  return inTimeline(project, timeline, (t) => {
    const tempo = clampTempo({ ...t.tempo, ...patch });
    return tempo.bpm === t.tempo.bpm && tempo.beatsPerBar === t.tempo.beatsPerBar && tempo.click === t.tempo.click
      ? t
      : { ...t, tempo };
  });
}


/** Records the tempo of an audio file; `null` forgets it, stored as no value. */
export function setAssetBpm(project: Project, assetId: string, bpm: number | null): Project {
  const target = project.assets.find((asset) => asset.id === assetId);
  const next = bpm === null ? null : clampTempo({ ...DEFAULT_TEMPO, bpm }).bpm;
  if (!target || (target.bpm ?? null) === next) return project;
  const assets = project.assets.map((asset) => {
    if (asset.id !== assetId) return asset;
    const { bpm: _previous, ...rest } = asset;
    return next === null ? rest : { ...rest, bpm: next };
  });
  return { ...project, assets };
}

export function setName(project: Project, name: string): Project {
  return { ...project, name };
}
