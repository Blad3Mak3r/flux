import { fail } from "../state/fail";
import { showLockGet, showLockSet } from "./api";
import { useShowLock } from "./lockStore";

/** Reads the lock from the backend, which keeps it for as long as the app runs. */
export async function loadShowLock(): Promise<void> {
  try {
    useShowLock.setState({ locked: await showLockGet() });
  } catch (err) {
    fail(err);
  }
}

/** Asks the backend to lock or unlock the show and shows the state it answers with. */
export async function setShowLock(locked: boolean): Promise<void> {
  try {
    useShowLock.setState({ locked: await showLockSet(locked) });
  } catch (err) {
    fail(err);
  }
}
