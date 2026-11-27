// The inspector's controls of a Live input generator: which camera or capture card it shows. The backend lists the devices
// and opens the one the clip names; a device that is not there is a normal state, shown as "not connected", never an error.

import { useCallback, useEffect, useState } from "react";
import { useT } from "../i18n";
import type { VjCaptureDevice, VjGenerator } from "../project/types";
import { vjCaptureDevices } from "./api";
import { edit } from "./store";

type Live = Extract<VjGenerator, { kind: "liveInput" }>;

export function LiveInputFields({ clip, generator }: { clip: string; generator: Live }) {
  const t = useT();
  const device = generator.device ?? "";
  // `null` until the first answer, so "not connected" is not claimed before the backend has been asked.
  const [devices, setDevices] = useState<VjCaptureDevice[] | null>(null);
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(() => {
    setBusy(true);
    return vjCaptureDevices()
      .catch(() => [] as VjCaptureDevice[])
      .then((list) => setDevices(list))
      .finally(() => setBusy(false));
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);

  const known = devices?.some((d) => d.name === device) ?? true;
  const missing = device !== "" && !known;
  const keep = (next: string) => void edit({ op: "clipGenerator", clip, generator: { ...generator, device: next } });

  return (
    <>
      <div className="vj-f"><span>{t("vj.gen.device")}</span>
        <div className="vj-segs vj-linestep">
          <select className="vj-select wide" value={device} aria-label={t("vj.gen.device")} onChange={(event) => keep(event.target.value)}>
            <option value="">{t("vj.gen.deviceNone")}</option>
            {missing && <option value={device}>{device}</option>}
            {devices?.map((d) => <option key={d.name} value={d.name}>{d.name === "default" ? t("vj.gen.deviceDefault") : d.label}</option>)}
          </select>
          <button type="button" disabled={busy} title={t("vj.gen.deviceRefresh")} aria-label={t("vj.gen.deviceRefresh")} onClick={() => void refresh()}>↻</button>
        </div>
      </div>
      {missing && <div className="vj-f"><span /><span className="vj-dim" role="status">{t("vj.gen.deviceMissing")}</span></div>}
      {devices?.length === 0 && device === "" && <div className="vj-f"><span /><span className="vj-dim" role="status">{t("vj.gen.deviceEmpty")}</span></div>}
    </>
  );
}
