// Whether the operator has locked the show. The backend holds the lock and refuses what it covers;
// this mirrors it so the window can show it and keep its own controls quiet.

import { create } from "zustand";

export const useShowLock = create<{ locked: boolean }>()(() => ({ locked: false }));

export const isShowLocked = () => useShowLock.getState().locked;
