import { fail } from "./fail";
import { outputSignalEdit } from "../project/api";
import type { SignalOp } from "../project/api";
import { dispatch, useDoc } from "./store";

let pending = Promise.resolve();
export function signalEdit(op: SignalOp): void {
  pending = pending.then(async () => {
    const project = useDoc.getState().project;
    const state = await outputSignalEdit(project, op);
    if (useDoc.getState().project.meta.id !== project.meta.id) return;
    dispatch({ type: "apply", ...state });
  }).catch((err) => fail(err));
}
