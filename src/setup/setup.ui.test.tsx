import { beforeEach, describe, expect, it } from "vitest";
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { usePreferences } from "../preferences/store";
import { closeSetup, openSetup } from "../state/ui";
import { renderApp, resetStores, settle } from "../test/app";
import { answer, calls } from "../test/tauri";

const transport = () => within(screen.getByRole("region", { name: "Transport" }));

describe("Setup with a device that is not connected", () => {
  beforeEach(() => {
    resetStores();
    answer("devices_list_audio", [{ name: "Studio card", outputChannels: 2, sampleRates: [48000], isDefault: true }]);
    const saved = usePreferences.getState().saved;
    usePreferences.setState({ saved: { ...saved, audio: { deviceName: "Ghost card", channels: [1, 2] } } });
  });

  it("names the missing device and offers the connected ones in its place", async () => {
    await renderApp();
    act(() => openSetup("audio"));
    await settle();

    const dialog = screen.getByRole("dialog", { name: "Preferences" });
    const device = within(dialog).getByRole("button", { name: "Audio device" });
    expect(device.textContent).toContain("Ghost card (not connected)");
    await userEvent.click(device);
    // The list the replacement is chosen from: the connected device is there.
    await userEvent.click(await screen.findByText("Studio card"));
    expect(calls("preferences_change").map((args) => JSON.stringify(args.change))).toEqual([expect.stringContaining("Studio card")]);
  });

  it("does not block the project: Setup closes and the transport still answers", async () => {
    await renderApp();
    act(() => openSetup("audio"));
    await settle();
    act(() => closeSetup());
    await settle();
    await userEvent.click(transport().getByRole("button", { name: /Play/ }));
    await settle();
    expect(calls("transport_play")).toHaveLength(1);
  });
});
