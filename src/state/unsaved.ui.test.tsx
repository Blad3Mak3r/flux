import { beforeEach, describe, expect, it } from "vitest";
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { open } from "@tauri-apps/plugin-dialog";
import { makeDirty, renderApp, resetStores, settle } from "../test/app";
import { calls } from "../test/tauri";

const press = (keys: string) => userEvent.keyboard(keys);
const dialog = () => screen.queryByRole("dialog", { name: /Save changes to/ });

describe("Unsaved changes", () => {
  beforeEach(resetStores);

  it("closing a project with no changes does not ask", async () => {
    await renderApp();
    await press("{Control>}w{/Control}");
    await settle();
    expect(dialog()).toBeNull();
    expect(calls("window_close_editor")).toHaveLength(1);
  });

  it("closing with changes asks first, and Cancel keeps the project open", async () => {
    await renderApp();
    makeDirty();
    await press("{Control>}w{/Control}");
    await settle();
    expect(dialog()).not.toBeNull();
    expect(calls("window_close_editor")).toHaveLength(0);

    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await settle();
    expect(dialog()).toBeNull();
    expect(calls("window_close_editor")).toHaveLength(0);
  });

  it("Don't save throws the changes away and closes", async () => {
    await renderApp();
    makeDirty();
    await press("{Control>}w{/Control}");
    await settle();
    await userEvent.click(screen.getByRole("button", { name: "Don't save" }));
    await settle();
    expect(dialog()).toBeNull();
    expect(calls("window_close_editor")).toHaveLength(1);
  });

  it("opening another project with changes asks before the file chooser appears", async () => {
    await renderApp();
    makeDirty();
    await press("{Control>}o{/Control}");
    await settle();
    expect(dialog()).not.toBeNull();
    expect(open).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole("button", { name: "Don't save" }));
    await settle();
    expect(open).toHaveBeenCalledTimes(1);
  });
});
