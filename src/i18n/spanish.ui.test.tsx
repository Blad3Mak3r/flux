import { beforeEach, describe, expect, it } from "vitest";
import { act, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { en } from "./en";
import { es } from "./es";
import { makeDirty, renderApp, resetStores, settle } from "../test/app";
import { answer } from "../test/tauri";
import { openSetup } from "../state/ui";

/** Every word the window shows or says to a screen reader, dialogs that are closed included. */
function shown(): string {
  const parts = [document.body.textContent ?? ""];
  document.body.querySelectorAll("[aria-label], [title]").forEach((el) => parts.push(el.getAttribute("aria-label") ?? "", el.getAttribute("title") ?? ""));
  return parts.join("\n");
}

/** A message key that reached the screen instead of its text (or a hole where a translation is missing). */
function leaks(text: string): string[] {
  const known = new Set(Object.keys(en));
  const keys = text.match(/[a-z][A-Za-z0-9]*(?:\.[A-Za-z0-9]+)+/g) ?? [];
  return [...new Set(keys.filter((key) => known.has(key)))].concat(text.includes("undefined") ? ["undefined"] : []);
}

describe("Spanish", () => {
  beforeEach(() => {
    resetStores();
    answer("live_cues", []);
  });

  it("the editor and its transport are in Spanish", async () => {
    await renderApp("es");
    expect(screen.getByRole("button", { name: new RegExp(es["transport.play"]) })).toBeTruthy();
    expect(screen.getByRole("group", { name: es["view.label"] })).toBeTruthy();
    expect(leaks(shown())).toEqual([]);
  });

  it("Live is in Spanish", async () => {
    await renderApp("es");
    await userEvent.keyboard("{Control>}2{/Control}");
    await settle();
    expect(screen.getByRole("region", { name: es["live.legend"] })).toBeTruthy();
    expect(leaks(shown())).toEqual([]);
  });

  it("the pages of Preferences are in Spanish", async () => {
    await renderApp("es");
    for (const page of ["project", "audio", "audioOutputs", "ltc", "mtc", "osc", "general"] as const) {
      act(() => openSetup(page));
      await settle();
      expect(screen.getByRole("dialog", { name: es["setup.title"] })).toBeTruthy();
      expect(leaks(shown()), page).toEqual([]);
    }
  });

  it("the unsaved changes dialog is in Spanish", async () => {
    await renderApp("es");
    makeDirty();
    await userEvent.keyboard("{Control>}w{/Control}");
    await settle();
    expect(screen.getByRole("button", { name: es["dialog.discard"] })).toBeTruthy();
    expect(screen.getByRole("button", { name: es["dialog.cancel"] })).toBeTruthy();
    expect(leaks(shown())).toEqual([]);
  });
});
