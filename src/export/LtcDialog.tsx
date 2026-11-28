import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import { useT } from "../i18n";
import { Select } from "../components/Select";
import { errorMessage } from "../project/api";
import { useDoc } from "../state/store";
import { say, sayError } from "../state/ui";
import { exportLtc, LTC_DEFAULTS, LTC_DEPTHS, LTC_MAX_PRE_ROLL, LTC_SAMPLE_RATES } from "./api";
import type { LtcDepth, LtcOptions } from "./api";

const useLtcDialog = create<{ open: boolean }>()(() => ({ open: false }));

/** Opens the LTC export dialog; the backend renders the file when the user confirms. */
export const openLtcDialog = () => useLtcDialog.setState({ open: true });
const close = () => useLtcDialog.setState({ open: false });

/** File > Export > LTC to WAV…: the format of the file before the save dialog asks where. */
export function LtcDialog() {
  const t = useT();
  const open = useLtcDialog((s) => s.open);
  const dialog = useRef<HTMLDialogElement>(null);
  const [sampleRate, setSampleRate] = useState(LTC_DEFAULTS.sampleRate);
  const [depth, setDepth] = useState<LtcDepth>(LTC_DEFAULTS.depth);
  const [preRoll, setPreRoll] = useState(String(LTC_DEFAULTS.preRollSeconds));
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open) dialog.current?.showModal();
    else dialog.current?.close();
  }, [open]);

  // Only digits are kept; the backend refuses a pre-roll out of range, so no rule lives here.
  const options = (): LtcOptions => ({ sampleRate, depth, preRollSeconds: Number(preRoll) || 0 });

  const run = async () => {
    setBusy(true);
    try {
      const frames = await exportLtc(useDoc.getState().project, options(), (fraction) =>
        say("status.ltcExporting", { percent: Math.round(fraction * 100) }),
      );
      if (frames !== null) {
        say("status.ltcExported");
        close();
      }
    } catch (err) {
      sayError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <dialog ref={dialog} className="dialog ltc-dialog" aria-labelledby="ltc-dialog-title" onClose={close}>
      <h2 id="ltc-dialog-title">{t("ltcExport.title")}</h2>
      <p>{t("ltcExport.intro")}</p>
      <div className="field">
        <label htmlFor="ltc-rate">{t("ltcExport.sampleRate")}</label>
        <Select id="ltc-rate" value={String(sampleRate)} onChange={(value) => setSampleRate(Number(value))}>
          {LTC_SAMPLE_RATES.map((rate) => (
            <option key={rate} value={rate}>{t("ltcExport.hz", { rate: rate / 1000 })}</option>
          ))}
        </Select>
      </div>
      <div className="field">
        <label htmlFor="ltc-depth">{t("ltcExport.depth")}</label>
        <Select id="ltc-depth" value={depth} onChange={(value) => setDepth(value as LtcDepth)}>
          {LTC_DEPTHS.map((item) => (
            <option key={item} value={item}>{t(`ltcExport.depth.${item}`)}</option>
          ))}
        </Select>
      </div>
      <div className="field">
        <label htmlFor="ltc-pre-roll">{t("ltcExport.preRoll")}</label>
        <input
          id="ltc-pre-roll"
          type="number"
          inputMode="numeric"
          min={0}
          max={LTC_MAX_PRE_ROLL}
          value={preRoll}
          onChange={(event) => setPreRoll(event.target.value)}
        />
        <span className="field-note">{t("ltcExport.preRollNote", { max: LTC_MAX_PRE_ROLL })}</span>
      </div>
      <div className="button-row">
        <button type="button" className="button primary" disabled={busy} onClick={() => void run()}>
          {t("ltcExport.export")}
        </button>
        <button type="button" className="button" onClick={close}>
          {t("ltcExport.cancel")}
        </button>
      </div>
    </dialog>
  );
}
