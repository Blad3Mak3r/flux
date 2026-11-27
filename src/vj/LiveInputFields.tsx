// The inspector's controls of a Live input generator: which camera or capture card it shows. The backend lists the devices
// and opens the one the clip names; a device that is not there is a normal state, shown as "not connected", never an error.

import { useCallback, useEffect, useState } from "react";
import { useT } from "../i18n";
import type { LiveRate, LiveSize, VjCaptureDevice, VjCaptureStatus, VjGenerator } from "../project/types";
import { vjCaptureDevices, vjCaptureStatus } from "./api";
import { edit } from "./store";

type Live = Extract<VjGenerator, { kind: "liveInput" }>;

const SIZES: LiveSize[] = ["auto", "1280x720", "1920x1080", "3840x2160"];
const RATES: LiveRate[] = ["auto", "25", "30", "50", "59.94", "60"];
// How often the backend is asked whether the device is open, busy, or refused the format.
const STATUS_MS = 1500;

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

  const [status, setStatus] = useState<VjCaptureStatus | null>(null);
  useEffect(() => {
    setStatus(null);
    if (device === "") return;
    let live = true;
    const ask = () => void vjCaptureStatus(device).then((next) => live && setStatus(next)).catch(() => undefined);
    ask();
    const timer = window.setInterval(ask, STATUS_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [device]);

  const known = devices?.some((d) => d.name === device) ?? true;
  const missing = device !== "" && !known;
  const change = (patch: Partial<Live>) => void edit({ op: "clipGenerator", clip, generator: { ...generator, ...patch } });
  const keep = (next: string) => change({ device: next });

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
      {status?.state === "busy" && <div className="vj-f"><span /><span className="vj-dim" role="status">{t("vj.gen.deviceBusy")}</span></div>}
      {missing && <div className="vj-f"><span /><span className="vj-dim" role="status">{t("vj.gen.deviceMissing")}</span></div>}
      {devices?.length === 0 && device === "" && <div className="vj-f"><span /><span className="vj-dim" role="status">{t("vj.gen.deviceEmpty")}</span></div>}
      <div className="vj-f"><span>{t("vj.gen.resolution")}</span>
        <select className="vj-select wide" value={generator.size ?? "auto"} aria-label={t("vj.gen.resolution")} onChange={(event) => change({ size: event.target.value as LiveSize })}>
          {SIZES.map((size) => <option key={size} value={size}>{size === "auto" ? t("vj.gen.auto") : size.replace("x", " × ")}</option>)}
        </select>
      </div>
      <div className="vj-f"><span>{t("vj.gen.fps")}</span>
        <select className="vj-select wide" value={generator.rate ?? "auto"} aria-label={t("vj.gen.fps")} onChange={(event) => change({ rate: event.target.value as LiveRate })}>
          {RATES.map((rate) => <option key={rate} value={rate}>{rate === "auto" ? t("vj.gen.auto") : rate}</option>)}
        </select>
      </div>
      {status?.refused && <div className="vj-f"><span /><span className="vj-dim" role="status">{t("vj.gen.formatRefused")}</span></div>}
    </>
  );
}
