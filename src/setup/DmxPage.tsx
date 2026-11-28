import { PlusIcon } from "../components/icons";
import { SelectField, TextField } from "../components/fields";
import { useT } from "../i18n";
import type { DmxDestination, DmxProtocol } from "../project/types";
import { addDmxDestination, removeDmxDestination, updateDmxDestination } from "../state/dmx";
import { useDoc } from "../state/store";
import type { CueInfo } from "../transport/api";
import { useTransport } from "../transport/store";
import { OutputRow, rowState } from "./pages";

const NO_DESTINATIONS: DmxDestination[] = [];
const NO_CUES: CueInfo[] = [];
const PROTOCOLS: DmxProtocol[] = ["artnet", "sacn"];

/** The DMX receivers the DMX tracks choose from. The backend adds, checks and removes them. */
export function DmxPage() {
  const t = useT();
  const destinations = useDoc((doc) => doc.project.output.dmx ?? NO_DESTINATIONS);
  const reported = useTransport((transport) => transport.info?.cues ?? NO_CUES);
  return (
    <div className="setup-fields">
      <p className="setup-intro">{t("setup.dmx.intro")}</p>
      {destinations.length === 0 && <p className="empty-note">{t("setup.dmx.empty")}</p>}
      <ul className="setup-rows">
        {destinations.map((destination) => {
          const result = reported.find((cue) => cue.kind === "dmx" && cue.target === destination.name);
          const save = (patch: Partial<DmxDestination>) => {
            const next = { ...destination, ...patch };
            void updateDmxDestination(destination.id, next.name, next.protocol, next.host);
          };
          return (
            <OutputRow
              key={destination.id}
              name={destination.name}
              {...rowState(result, true)}
              onRemove={() => void removeDmxDestination(destination.id)}
            >
              <TextField label={`${destination.name}: ${t("setup.dmx.name")}`} value={destination.name} onCommit={(name) => save({ name })} />
              <SelectField
                label={`${destination.name}: ${t("setup.dmx.protocol")}`}
                value={destination.protocol}
                onChange={(protocol) => save({ protocol: protocol as DmxProtocol })}
              >
                {PROTOCOLS.map((protocol) => (
                  <option key={protocol} value={protocol}>
                    {t(`setup.dmx.protocol.${protocol}`)}
                  </option>
                ))}
              </SelectField>
              <TextField
                label={`${destination.name}: ${t("setup.dmx.host")}`}
                hint={t("setup.dmx.host.hint")}
                value={destination.host}
                monospace
                onCommit={(host) => save({ host })}
              />
            </OutputRow>
          );
        })}
      </ul>
      <div className="button-row">
        <button
          type="button"
          className="button"
          onClick={() => void addDmxDestination(t("setup.dmx.default", { number: destinations.length + 1 }), "artnet", "255.255.255.255")}
        >
          <PlusIcon />
          {t("setup.dmx.add")}
        </button>
      </div>
    </div>
  );
}
