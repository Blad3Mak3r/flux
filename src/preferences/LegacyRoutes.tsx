import { useState } from "react";
import { useT } from "../i18n";
import { useDoc } from "../state/store";
import { changePreferences, usePreferences } from "./store";
import type { OutputConfig } from "../project/types";

export function LegacyRoutes() {
  const t = useT();
  const routes = useDoc((d) => d.project.output.legacyRoutes);
  const saved = usePreferences((p) => p.saved);
  const [review, setReview] = useState(false);
  if (!routes) return null;
  const audio = (r: OutputConfig["audio"] | undefined) =>
    r
      ? `${r.deviceName || t("output.default")} · ${r.channels.join(" / ")}`
      : t("setup.unassigned");
  const ltc = (r: OutputConfig["ltc"][number] | undefined) =>
    r
      ? `${r.deviceName || t("output.default")} · ${t("output.channel", { channel: r.channel })} · ${r.offsetMs ?? 0} ms`
      : t("setup.unassigned");
  const mtc = (r: OutputConfig["mtc"][number] | undefined) =>
    r
      ? `${r.portName} (${(r.portIndex ?? 0) + 1}) · ${r.offsetMs ?? 0} ms`
      : t("setup.unassigned");
  const rows = [
    ...(routes.driver
      ? [[t("out.driver"), saved.driver || t("output.default"), routes.driver]]
      : []),
    ...(routes.audio
      ? [[t("out.audioDevice"), audio(saved.audio), audio(routes.audio)]]
      : []),
    ...(routes.preview
      ? [[t("out.previewDevice"), audio(saved.preview), audio(routes.preview)]]
      : []),
    ...(routes.metronome
      ? [
          [
            t("out.metronomeDevice"),
            audio(saved.metronome),
            audio(routes.metronome),
          ],
        ]
      : []),
    ...routes.ltc.map((r, i) => [
      t("setup.ltc.name", { number: i + 1 }),
      ltc(saved.ltc[i + 1]),
      ltc(r),
    ]),
    ...routes.mtc.map((r, i) => [
      t("setup.mtc.name", { number: i + 1 }),
      mtc(saved.mtc[i + 1]),
      mtc(r),
    ]),
  ];
  if (!rows.length) return null;
  return (
    <section className="legacy-routes">
      <p className="field-note">{t("setup.import.note")}</p>
      {!review ? (
        <button
          type="button"
          className="button"
          onClick={() => setReview(true)}
        >
          {t("setup.import.action")}
        </button>
      ) : (
        <>
          <table>
            <thead>
              <tr>
                <th>{t("setup.import.output")}</th>
                <th>{t("setup.import.before")}</th>
                <th>{t("setup.import.after")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(([name, before, after]) => (
                <tr key={name}>
                  <th>{name}</th>
                  <td>{before}</td>
                  <td>{after}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="button-row">
            <button
              type="button"
              className="button"
              onClick={() => setReview(false)}
            >
              {t("dialog.cancel")}
            </button>
            <button
              type="button"
              className="button primary"
              onClick={() => {
                void changePreferences({ op: "import", routes }).then(
                  () => setReview(false),
                  () => undefined,
                );
              }}
            >
              {t("setup.import.confirm")}
            </button>
          </div>
        </>
      )}
    </section>
  );
}
