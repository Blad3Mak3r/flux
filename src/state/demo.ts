// A synthetic show so the interface has something to work on. The audio does not exist: the
// backend has never seen these assets, so the demo can be edited and played as timecode but not
// saved.

import type { Project } from "../project/types";
import type { AssetAudio } from "./doc";

const FPS = 25;
const seconds = (s: number) => s * FPS;

export function demoShow(name: string): { project: Project; audio: Record<string, AssetAudio> } {
  const project: Project = {
    formatVersion: 1,
    name,
    timecode: { frameRate: "fps25", start: 1 * 3_600 * FPS },
    assets: [
      { id: "demo-intro", name: "Intro bed", file: "audio/demo-intro.wav" },
      { id: "demo-song", name: "Song 1 backing", file: "audio/demo-song.wav" },
      { id: "demo-click", name: "Click", file: "audio/demo-click.wav" },
    ],
    tracks: [
      {
        id: "demo-t-intro",
        name: "Intro bed",
        muted: false,
        kind: {
          type: "audio",
          clips: [{ id: "demo-c-intro", asset: "demo-intro", start: 0, gainDb: -6 }],
        },
      },
      {
        id: "demo-t-song",
        name: "Song 1 backing",
        muted: false,
        kind: {
          type: "audio",
          clips: [{ id: "demo-c-song", asset: "demo-song", start: seconds(8), gainDb: 0 }],
        },
      },
      {
        id: "demo-t-click",
        name: "Click",
        muted: true,
        kind: {
          type: "audio",
          clips: [{ id: "demo-c-click", asset: "demo-click", start: seconds(8), gainDb: -12 }],
        },
      },
      {
        id: "demo-t-cues",
        name: "Lighting cues",
        muted: false,
        kind: {
          type: "events",
          triggers: [
            { id: "demo-e1", at: 0, label: "House to half", action: { type: "marker" } },
            { id: "demo-e2", at: seconds(8), label: "Song 1 look", action: { type: "midi", bytes: [0xc0, 0x05] } },
            { id: "demo-e3", at: seconds(15), label: "Verse", action: { type: "marker" } },
            { id: "demo-e3b", at: seconds(20), label: "Colour wash", action: { type: "midi", bytes: [0xc0, 0x07] } },
            { id: "demo-e4", at: seconds(60), label: "Bridge", action: { type: "midi", bytes: [0xc0, 0x06] } },
            { id: "demo-e5", at: seconds(110), label: "Last chorus", action: { type: "marker" } },
            { id: "demo-e6", at: seconds(198), label: "Blackout", action: { type: "midi", bytes: [0xc0, 0x00] } },
          ],
        },
      },
    ],
    output: { audio: null, ltc: null, mtc: null },
  };
  return {
    project,
    audio: {
      "demo-intro": { seconds: 14 },
      "demo-song": { seconds: 190 },
      "demo-click": { seconds: 190 },
    },
  };
}
