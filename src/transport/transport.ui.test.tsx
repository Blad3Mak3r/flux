import { beforeEach, describe, expect, it } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { renderApp, resetStores, settle } from "../test/app";
import { calls, commands, send } from "../test/tauri";
import type { Position } from "./api";

const position = (patch: Partial<Position>): Position => ({
  state: "playing",
  frame: 0,
  positionSamples: 0,
  deviceLost: false,
  ltcLost: 0,
  mtcLost: 0,
  metronomeLost: false,
  operatorLost: false,
  clockUnpaced: false,
  ...patch,
});

const transport = () => within(screen.getByRole("region", { name: "Transport" }));

describe("Transport", () => {
  beforeEach(resetStores);

  it("Play opens the output and starts the clock", async () => {
    await renderApp();
    await userEvent.click(transport().getByRole("button", { name: /Play/ }));
    await settle();
    expect(commands()).toContain("output_start");
    expect(commands()[commands().length - 1]).toBe("transport_play");
  });

  it("Pause and Stop send their own commands", async () => {
    await renderApp();
    await userEvent.click(transport().getByRole("button", { name: /Play/ }));
    await settle();
    act(() => send("output_start", "onPosition", position({ state: "playing", frame: 10 })));

    await userEvent.click(transport().getByRole("button", { name: /Pause/ }));
    expect(calls("transport_pause")).toHaveLength(1);

    await userEvent.click(transport().getByRole("button", { name: /Stop/ }));
    await settle();
    expect(calls("transport_stop")).toHaveLength(1);
  });

  it("the timecode follows the position the backend sends", async () => {
    await renderApp();
    await userEvent.click(transport().getByRole("button", { name: /Play/ }));
    await settle();
    expect(screen.getByLabelText(/^Timecode 00:00:00:00/)).toBeTruthy();

    // 25 fps: 25 * 3600 + 25 * 60 + 25 * 5 + 7 frames is 01:01:05:07.
    act(() => send("output_start", "onPosition", position({ frame: 25 * 3665 + 7 })));
    expect(screen.getByLabelText(/^Timecode 01:01:05:07/)).toBeTruthy();
    expect(transport().getByRole("button", { name: /Play/ }).getAttribute("aria-pressed")).toBe("true");

    act(() => send("output_start", "onPosition", position({ state: "paused", frame: 25 * 3665 + 7 })));
    expect(transport().getByRole("button", { name: /Pause/ }).getAttribute("aria-pressed")).toBe("true");
  });
});
