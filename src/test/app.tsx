// Renders the real app (outside the desktop window, so there is no backend to start from) with the
// preferences the backend would have sent: the language and the keys.

import { act, render } from "@testing-library/react";
import App from "../App";
import type { ActionView } from "../keymap/keys";
import type { Locale } from "../i18n/messages";
import { usePreferences } from "../preferences/store";
import { dispatch, useDoc } from "../state/store";
import { useUi } from "../state/ui";
import { useTransport } from "../transport/store";
import { useLive } from "../live/store";

const KEYS: Partial<Record<ActionView["id"], string>> = {
  playPause: "Space",
  stop: "Enter",
  viewEdit: "Mod+1",
  viewLive: "Mod+2",
  open: "Mod+O",
  close: "Mod+W",
  setup: "Mod+,",
};

const keymap: ActionView[] = Object.entries(KEYS).map(([id, key]) => ({
  id: id as ActionView["id"],
  group: "view",
  keys: [key],
  defaults: [key],
  locked: id === "open" || id === "close",
  editOnly: false,
}));

export async function renderApp(locale: Locale = "en") {
  const saved = usePreferences.getState().saved;
  usePreferences.setState({ saved: { ...saved, locale, keymap }, ready: true });
  const view = render(<App />);
  await settle();
  return view;
}

/** Lets what the UI started (promises, effects) finish. */
export const settle = () => act(async () => void (await new Promise((resolve) => setTimeout(resolve, 0))));

/** Puts the stores back as a fresh window has them. */
export function resetStores() {
  useUi.setState(useUi.getInitialState(), true);
  useTransport.setState(useTransport.getInitialState(), true);
  useLive.setState(useLive.getInitialState(), true);
  useDoc.setState(useDoc.getInitialState(), true);
}

/** Gives the open project an unsaved change. */
export function makeDirty() {
  act(() => {
    const { project } = useDoc.getState();
    dispatch({ type: "edit", project: { ...project, name: `${project.name} (edited)` } });
  });
}
