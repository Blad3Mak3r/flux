// Spanish: one module per area in ./es/, the same areas as ./en/. TypeScript fails the build if a key is missing.
import type { MessageKey } from "./en.ts";
import { voice } from "./es/voice.ts";
import { live } from "./es/live.ts";
import { timeline } from "./es/timeline.ts";
import { setup } from "./es/setup.ts";
import { chrome } from "./es/chrome.ts";
import { outputs } from "./es/outputs.ts";
import { show } from "./es/show.ts";
import { media } from "./es/media.ts";
import { events } from "./es/events.ts";
import { vjGrid } from "./es/vj-grid.ts";
import { vjTransport } from "./es/vj-transport.ts";
import { vjEffects } from "./es/vj-effects.ts";
import { vjMenu } from "./es/vj-menu.ts";
import { vjMapping } from "./es/vj-mapping.ts";
import { vjOutput } from "./es/vj-output.ts";

export const es: Record<MessageKey, string> = {
  ...voice,
  ...live,
  ...timeline,
  ...setup,
  ...chrome,
  ...outputs,
  ...show,
  ...media,
  ...events,
  ...vjGrid,
  ...vjTransport,
  ...vjEffects,
  ...vjMenu,
  ...vjMapping,
  ...vjOutput,
};
