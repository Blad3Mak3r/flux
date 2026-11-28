// Runs before every component test: the Tauri API is replaced by `./tauri`, and the few browser
// features jsdom lacks (modal dialogs, observers) get a stand-in.

import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { resetTauri } from "./tauri";

vi.mock("@tauri-apps/api/core", async () => {
  const fake = await import("./tauri");
  return { invoke: fake.invoke, Channel: fake.Channel, isTauri: () => false, convertFileSrc: (path: string) => path };
});
vi.mock("@tauri-apps/api/event", async () => {
  const fake = await import("./tauri");
  return { listen: fake.listen, emit: async () => undefined };
});
vi.mock("@tauri-apps/api/window", async () => {
  const fake = await import("./tauri");
  return { getCurrentWindow: () => fake.fakeWindow };
});
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(async () => null), save: async () => null, message: async () => undefined, ask: async () => false }));

class Quiet {
  observe() {}
  unobserve() {}
  disconnect() {}
}
globalThis.ResizeObserver ??= Quiet as unknown as typeof ResizeObserver;
globalThis.IntersectionObserver ??= Quiet as unknown as typeof IntersectionObserver;
window.matchMedia ??= ((query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {}, onchange: null, dispatchEvent: () => false })) as typeof window.matchMedia;

Element.prototype.scrollIntoView ??= () => undefined;

// jsdom has <dialog> but not showModal/close.
HTMLDialogElement.prototype.showModal ??= function (this: HTMLDialogElement) {
  this.setAttribute("open", "");
};
HTMLDialogElement.prototype.show ??= HTMLDialogElement.prototype.showModal;
HTMLDialogElement.prototype.close ??= function (this: HTMLDialogElement) {
  this.removeAttribute("open");
};

beforeEach(() => {
  vi.clearAllMocks();
  resetTauri();
  localStorage.clear();
});
afterEach(() => {
  cleanup();
});
