// English is the default language and the source of truth for the message keys. One module per area in ./en/.
import { voice } from "./en/voice.ts";
import { live } from "./en/live.ts";
import { timeline } from "./en/timeline.ts";
import { setup } from "./en/setup.ts";
import { chrome } from "./en/chrome.ts";
import { outputs } from "./en/outputs.ts";
import { show } from "./en/show.ts";
import { media } from "./en/media.ts";
import { events } from "./en/events.ts";
import { vjGrid } from "./en/vj-grid.ts";
import { vjTransport } from "./en/vj-transport.ts";
import { vjEffects } from "./en/vj-effects.ts";
import { vjMenu } from "./en/vj-menu.ts";
import { vjMapping } from "./en/vj-mapping.ts";
import { vjOutput } from "./en/vj-output.ts";

export const en = {
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
} as const;

export type MessageKey = keyof typeof en;
