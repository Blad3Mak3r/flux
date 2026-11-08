import { useI18n } from "../i18n";
import type { EntryOverview } from "./types";

/** Only translates the facts supplied by the backend; it does not inspect timeline controls. */
export function EntrySummary({ entry }: { entry: EntryOverview }) {
  const { t, locale } = useI18n();
  return <span className="entry-summary">
    {entry.source && <span>{t("show.sourceSummary", { name: entry.source })}</span>}
    <span>{entry.transition.type === "after"
      ? t("show.summary.afterExact", { milliseconds: entry.transition.milliseconds.toLocaleString(locale) })
      : t(`show.summary.${entry.transition.type}`)}</span>
    {entry.manualWaits > 0 && <span>{t("show.summary.manualWaits", { count: entry.manualWaits })}</span>}
    {entry.indefiniteLoops > 0 && <span>{t("show.summary.indefiniteLoops", { count: entry.indefiniteLoops })}</span>}
    {entry.finiteLoops > 0 && <span>{t("show.summary.finiteLoops", { count: entry.finiteLoops })}</span>}
    {entry.omitted && <span>{t("show.omitted")}</span>}
  </span>;
}
