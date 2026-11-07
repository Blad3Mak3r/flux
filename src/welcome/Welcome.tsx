import { useCallback, useEffect, useState } from "react";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { BrandMark } from "../components/BrandMark";
import { FolderIcon, PlusIcon, TrashIcon } from "../components/icons";
import { WindowControls, hasNativeWindow } from "../components/WindowControls";
import { I18nProvider, useI18n } from "../i18n";
import { errorMessage, pickProjectPath } from "../project/api";
import { openEditor, openPending, pendingPath, projectDelete, recentsForget, recentsList, recoveryDiscard, recoveryList, recoveryOriginal, recoveryRestore } from "./api";
import { projectInitials } from "./initials";
import type { RecentShow, RecoverySummary } from "./api";

const basename = (path: string) => path.split(/[\\/]/).pop() ?? path;

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["day", 86_400],
  ["hour", 3_600],
  ["minute", 60],
];

/** "2 days ago" in the interface language. */
function ago(openedAt: number, locale: string): string {
  const seconds = Math.round(openedAt - Date.now() / 1000);
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const [unit, size] = UNITS.find(([, size]) => Math.abs(seconds) >= size) ?? ["minute", 60];
  return format.format(Math.round(seconds / size), unit);
}

function Welcome() {
  const { t, locale } = useI18n();
  const [shows, setShows] = useState<RecentShow[] | null>(null);
  const [asking, setAsking] = useState<RecentShow | null>(null);
  const [recoveries, setRecoveries] = useState<RecoverySummary[]>([]);
  const [discarding, setDiscarding] = useState<RecoverySummary | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(
    async () => {
      const [recent, recovery, requested] = await Promise.allSettled([recentsList(), recoveryList(), pendingPath()]);
      if (recent.status === "fulfilled") setShows(recent.value);
      else { setShows([]); setError(errorMessage(recent.reason)); }
      if (recovery.status === "fulfilled") setRecoveries(recovery.value);
      else setError(errorMessage(recovery.reason));
      if (requested.status === "fulfilled") setPending(requested.value);
    },
    [],
  );
  useEffect(() => {
    void reload();
  }, [reload]);

  // Opening a window twice would fail on its label, so one action runs at a time.
  const guarded = (action: () => Promise<void>) => async () => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const openPicked = guarded(async () => {
    const path = await pickProjectPath();
    if (path) await openEditor(path);
  });
  const openShow = (path: string) => guarded(() => openEditor(path))();
  const forget = (path: string) => guarded(() => recentsForget(path).then(reload))();
  const confirmDelete = guarded(async () => {
    if (!asking) return;
    const { path } = asking;
    setAsking(null);
    await projectDelete(path);
    await reload();
  });
  const confirmDiscard = guarded(async () => {
    if (!discarding) return;
    await recoveryDiscard(discarding.id);
    setDiscarding(null);
    await reload();
  });

  return (
    <div className="welcome">
      <nav className="menubar" aria-label={t("app.title")} data-tauri-drag-region>
        <BrandMark />
        <span className="welcome-titlebar-spacer" data-tauri-drag-region />
        {hasNativeWindow() && <WindowControls />}
      </nav>

      <main className="welcome-body">
        {error && <p className="message" data-tone="error" role="alert">{error}</p>}
        {recoveries.length > 0 && (
          <section aria-labelledby="recovery-legend">
            <h2 className="legend" id="recovery-legend">{t("recovery.title")}</h2>
            <p className="field-note">{t("recovery.intro")}</p>
            <ul className="recents">
              {recoveries.map((record) => (
                <li key={record.id} className="recovery-row">
                  <div className="recovery-detail">
                    <strong>{record.name ?? t("recovery.unreadable")}</strong>
                    <span className="recovery-path" title={record.path ?? undefined}>{record.path ?? t("recovery.unsaved")}</span>
                    {record.savedAt && <time dateTime={record.savedAt}>{new Date(record.savedAt).toLocaleString(locale)}</time>}
                    {!record.readable && <span className="field-note">{t("recovery.invalid")}</span>}
                  </div>
                  <div className="button-row recovery-actions">
                    <button type="button" className="button primary" disabled={busy || !record.readable}
                      onClick={() => void guarded(() => recoveryRestore(record.id))()}>{t("recovery.restore")}</button>
                    {record.path && <button type="button" className="button" disabled={busy || !record.readable}
                      onClick={() => void guarded(() => recoveryOriginal(record.id))()}>{t("recovery.original")}</button>}
                    <button type="button" className="button" disabled={busy}
                      onClick={() => setDiscarding(record)}>{t("recovery.discard")}</button>
                  </div>
                </li>
              ))}
            </ul>
          </section>
        )}
        {pending && <button type="button" className="button" disabled={busy}
          onClick={() => void guarded(openPending)()}>{t("recovery.pending", { name: basename(pending) })}</button>}
        <div className="button-row">
          <button type="button" className="button primary" disabled={busy} onClick={() => void guarded(() => openEditor(null))()}>
            <PlusIcon />
            {t("welcome.new")}
          </button>
          <button type="button" className="button" disabled={busy} onClick={() => void openPicked()}>
            <FolderIcon />
            {t("welcome.open")}
          </button>
        </div>

        <section aria-labelledby="recent-legend">
          <h2 className="legend" id="recent-legend">{t("welcome.recent")}</h2>
          {shows?.length === 0 ? (
            <div className="empty-state">
              <p className="empty-title">{t("welcome.empty.title")}</p>
              <p>{t("welcome.empty.body")}</p>
            </div>
          ) : (
            <ul className="recents">
              {shows?.map((show) => {
                const name = show.meta?.name ?? basename(show.path);
                const readable = show.meta !== null;
                return (
                  <li key={show.path} className="recent">
                    <button
                      type="button"
                      className="recent-open"
                      disabled={busy || !readable}
                      aria-label={t("welcome.openName", { name })}
                      onClick={() => openShow(show.path)}
                    >
                      <span className="project-badge" aria-hidden="true">{projectInitials(name)}</span>
                      <span className="recent-name">{name}</span>
                      <span className="recent-path" title={show.path}>{show.path}</span>
                      <span className="recent-meta">
                        {show.meta ? (
                          <>
                            <span className="mono">{t(`frameRate.${show.meta.frameRate}`)}</span>
                            <span className="mono">{t("welcome.tracks", { count: show.meta.trackCount })}</span>
                            <span>{ago(show.openedAt, locale)}</span>
                          </>
                        ) : (
                          <span className="recent-state">
                            {t(show.exists ? "welcome.unreadable" : "welcome.missing")}
                          </span>
                        )}
                      </span>
                    </button>
                    {show.exists ? (
                      <button
                        type="button"
                        className="icon-button"
                        disabled={busy}
                        aria-label={t("welcome.delete", { name })}
                        title={t("welcome.delete", { name })}
                        onClick={() => setAsking(show)}
                      >
                        <TrashIcon />
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="button"
                        disabled={busy}
                        aria-label={t("welcome.forgetName", { name })}
                        onClick={() => forget(show.path)}
                      >
                        {t("welcome.forget")}
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </section>

      </main>

      <ConfirmDialog
        open={asking !== null || discarding !== null}
        title={discarding ? t("recovery.discardTitle") : t("delete.title", { name: asking ? (asking.meta?.name ?? basename(asking.path)) : "" })}
        detail={discarding ? (discarding.name ?? t("recovery.unreadable")) : asking?.path}
        body={t(discarding ? "recovery.discardBody" : "delete.body")}
        confirmLabel={t(discarding ? "recovery.discard" : "delete.confirm")}
        onConfirm={() => { if (!busy) void (discarding ? confirmDiscard() : confirmDelete()); }}
        onCancel={() => { setAsking(null); setDiscarding(null); }}
      />
    </div>
  );
}

export default function WelcomeView() {
  return (
    <I18nProvider>
      <Welcome />
    </I18nProvider>
  );
}
