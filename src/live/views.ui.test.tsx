import { beforeEach, describe, expect, it } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useTransport } from "../transport/store";
import { renderApp, resetStores, settle } from "../test/app";
import { answer, calls } from "../test/tauri";
import type { Cue, LiveState } from "./api";

const cue = (number: number, name: string, frame: number, timecode: string): Cue => ({
  id: `cue-${number}`,
  number,
  kind: "audio",
  name,
  text: null,
  action: null,
  detail: null,
  trackId: "track-1",
  trackName: "Music",
  color: null,
  muted: false,
  frame,
  timecode,
  length: null,
  duration: null,
});

const CUES = [cue(1, "Opening", 0, "00:00:00:00"), cue(2, "Finale", 25 * 90, "00:01:30:00")];
const STATE: LiveState = { statuses: ["next", "pending"], current: null, next: null, running: [], notes: [] };

const view = (name: "Edit" | "Live") => within(screen.getByRole("group", { name: "View" })).getByRole("button", { name });

describe("Edit and Live views", () => {
  beforeEach(() => {
    resetStores();
    answer("live_cues", CUES);
    answer("live_state", STATE);
  });

  it("Ctrl+2 goes to Live and Ctrl+1 back to Edit", async () => {
    await renderApp();
    expect(view("Edit").getAttribute("aria-pressed")).toBe("true");
    expect(screen.queryByRole("complementary", { name: "Inspector" })).not.toBeNull();

    await userEvent.keyboard("{Control>}2{/Control}");
    await settle();
    expect(view("Live").getAttribute("aria-pressed")).toBe("true");
    expect(screen.queryByRole("region", { name: "Live" })).not.toBeNull();
    expect(screen.queryByRole("complementary", { name: "Inspector" })).toBeNull();

    await userEvent.keyboard("{Control>}1{/Control}");
    await settle();
    expect(view("Edit").getAttribute("aria-pressed")).toBe("true");
    expect(screen.queryByRole("region", { name: "Live" })).toBeNull();
  });

  it("the view cannot change while the show plays", async () => {
    await renderApp();
    act(() => useTransport.setState({ position: { state: "playing", frame: 5, positionSamples: 0, deviceLost: false, ltcLost: 0, mtcLost: 0, metronomeLost: false, operatorLost: false, clockUnpaced: false } }));
    expect((view("Live") as HTMLButtonElement).disabled).toBe(true);
    await userEvent.keyboard("{Control>}2{/Control}");
    await settle();
    expect(view("Edit").getAttribute("aria-pressed")).toBe("true");
  });

  it("Live shows the backend's cues, and only a double click goes to one", async () => {
    await renderApp();
    await userEvent.keyboard("{Control>}2{/Control}");
    await settle();
    // Nothing here is derived in the window: the cues are what live_cues answered.
    expect(calls("live_cues").length).toBeGreaterThan(0);
    const finale = await screen.findByRole("button", { name: /Finale/ });
    expect(screen.getByLabelText(/^Timecode 00:00:00:00/)).toBeTruthy();

    await userEvent.click(finale);
    expect(screen.getByLabelText(/^Timecode 00:00:00:00/)).toBeTruthy();

    await userEvent.dblClick(finale);
    expect(screen.getByLabelText(/^Timecode 00:01:30:00/)).toBeTruthy();
  });

  it("Live offers no way to edit the cues", async () => {
    await renderApp();
    await userEvent.keyboard("{Control>}2{/Control}");
    await settle();
    await screen.findByRole("button", { name: /Opening/ });
    const live = screen.getByRole("region", { name: "Live" });
    // Reading the show and running it: no field to type into, no track or clip controls.
    expect(within(live).queryAllByRole("textbox")).toHaveLength(0);
    expect(screen.queryByRole("complementary", { name: "Inspector" })).toBeNull();
    expect(calls("cue_edit")).toHaveLength(0);
    expect(calls("clip_edit")).toHaveLength(0);
  });
});
