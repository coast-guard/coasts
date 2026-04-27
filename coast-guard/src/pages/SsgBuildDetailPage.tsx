import { useCallback } from 'react';
import { useParams } from 'react-router';
import { useTranslation } from 'react-i18next';
import Editor, { type BeforeMount } from '@monaco-editor/react';
import { useSsgBuildInspect } from '../api/hooks';
import { useEditorTheme, ALL_EDITOR_THEMES } from '../hooks/useEditorTheme';
import { setupJsxSupport } from '../lib/monaco-jsx';
import Breadcrumb from '../components/Breadcrumb';

function relativeTime(unix: number, t: ReturnType<typeof useTranslation>['t']): string {
    if (unix <= 0) {
        return '—';
    }
    const seconds = Math.floor(Date.now() / 1000 - unix);
    if (seconds < 60) {
        return t('time.justNow');
    }
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60) {
        return minutes === 1
            ? t('time.minuteAgo')
            : t('time.minutesAgo', { count: minutes });
    }
    const hours = Math.floor(minutes / 60);
    if (hours < 24) {
        return hours === 1 ? t('time.hourAgo') : t('time.hoursAgo', { count: hours });
    }
    const days = Math.floor(hours / 24);
    if (days < 30) {
        return days === 1 ? t('time.dayAgo') : t('time.daysAgo', { count: days });
    }
    return t('time.monthsAgo', { count: Math.floor(days / 30) });
}

export default function SsgBuildDetailPage() {
    const { t } = useTranslation();
    const { project, buildId } = useParams<{ project: string; buildId: string }>();
    const { activeTheme } = useEditorTheme();
    const { data: inspect, isLoading, error } = useSsgBuildInspect(
        project ?? '',
        buildId ?? '',
    );

    const handleBeforeMount: BeforeMount = useCallback((monaco) => {
        setupJsxSupport(monaco, ALL_EDITOR_THEMES);
    }, []);

    const crumbs = [
        { label: t('nav.projects'), to: '/' },
        { label: project ?? '', to: `/project/${project}` },
        { label: t('build.ssgBuildsBreadcrumb'), to: `/project/${project}/builds` },
        { label: buildId ?? '' },
    ];

    return (
        <div className="page-shell">
            <Breadcrumb items={crumbs} />
            <div className="flex items-center gap-2 mb-4 flex-wrap">
                <h2 className="text-lg font-semibold text-main">
                    {t('build.buildId')}: <span className="font-mono">{buildId}</span>
                </h2>
                {inspect?.latest && (
                    <span className="px-1.5 py-0.5 rounded bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 font-mono text-[10px]">
                        {t('build.ssgLatestBadge')}
                    </span>
                )}
                {inspect?.pinned && (
                    <span className="px-1.5 py-0.5 rounded bg-amber-500/10 text-amber-600 dark:text-amber-400 font-mono text-[10px]">
                        {t('build.ssgPinnedBadge')}
                    </span>
                )}
            </div>

            {isLoading && (
                <div className="glass-panel p-6 text-sm text-subtle-ui">Loading…</div>
            )}

            {error != null && (
                <div className="glass-panel p-6 text-sm text-rose-600 dark:text-rose-400">
                    {error instanceof Error ? error.message : String(error)}
                </div>
            )}

            {inspect != null && (
                <section className="mt-1 space-y-4">
                    <div className="glass-panel p-5">
                        <h3 className="text-sm font-semibold text-main mb-3">
                            {t('build.metadata')}
                        </h3>
                        <div className="grid grid-cols-2 gap-y-2 gap-x-6 text-sm">
                            <span className="text-subtle-ui">{t('build.buildId')}</span>
                            <span className="text-main font-mono text-xs break-all">
                                {inspect.build_id}
                            </span>
                            <span className="text-subtle-ui">{t('build.coastfileHash')}</span>
                            <span className="text-main font-mono text-xs">
                                {inspect.coastfile_hash}
                            </span>
                            <span className="text-subtle-ui">{t('build.built')}</span>
                            <span className="text-main">
                                {relativeTime(inspect.built_at_unix, t)}
                                {inspect.built_at && (
                                    <span className="ml-2 text-xs text-subtle-ui font-mono">
                                        ({inspect.built_at})
                                    </span>
                                )}
                            </span>
                            <span className="text-subtle-ui">{t('build.artifact')}</span>
                            <span className="text-main font-mono text-xs break-all">
                                {inspect.artifact_path}
                            </span>
                        </div>
                    </div>

                    {inspect.services.length > 0 && (
                        <div className="glass-panel overflow-hidden">
                            <h3 className="text-sm font-semibold text-main px-5 pt-4 pb-2">
                                {t('build.ssgServicesHeader')} ({inspect.services.length})
                            </h3>
                            <div className="overflow-x-auto">
                                <table className="w-full text-sm">
                                    <thead>
                                        <tr className="border-b border-[var(--border)] text-left text-xs text-subtle-ui">
                                            <th className="px-5 py-2 font-medium">
                                                {t('col.name')}
                                            </th>
                                            <th className="px-4 py-2 font-medium">
                                                {t('build.image')}
                                            </th>
                                            <th className="px-4 py-2 font-medium">
                                                {t('build.ports')}
                                            </th>
                                            <th className="px-4 py-2 font-medium">
                                                {t('build.envKeys')}
                                            </th>
                                            <th className="px-4 py-2 font-medium">
                                                {t('build.volumes')}
                                            </th>
                                        </tr>
                                    </thead>
                                    <tbody className="divide-y divide-[var(--border)]">
                                        {inspect.services.map((svc) => (
                                            <tr key={svc.name}>
                                                <td className="px-5 py-2.5 font-mono text-xs text-main">
                                                    {svc.name}
                                                </td>
                                                <td className="px-4 py-2.5 font-mono text-xs">
                                                    {svc.image}
                                                </td>
                                                <td className="px-4 py-2.5 font-mono text-xs text-subtle-ui">
                                                    {svc.ports.length > 0
                                                        ? svc.ports.join(', ')
                                                        : '—'}
                                                </td>
                                                <td className="px-4 py-2.5 text-xs">
                                                    {svc.env_keys.length > 0 ? (
                                                        <div className="flex flex-wrap gap-1">
                                                            {svc.env_keys.map((k) => (
                                                                <span
                                                                    key={k}
                                                                    className="px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-700 dark:text-amber-300 font-mono text-[10px]"
                                                                >
                                                                    {k}
                                                                </span>
                                                            ))}
                                                        </div>
                                                    ) : (
                                                        <span className="text-subtle-ui">—</span>
                                                    )}
                                                </td>
                                                <td className="px-4 py-2.5 text-xs text-subtle-ui">
                                                    {svc.volumes.length > 0
                                                        ? svc.volumes.join(', ')
                                                        : '—'}
                                                </td>
                                            </tr>
                                        ))}
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    )}

                    {inspect.coastfile != null && (
                        <div className="glass-panel overflow-hidden">
                            <h3 className="text-sm font-semibold text-main px-5 pt-4 pb-2">
                                Coastfile.shared_service_groups
                            </h3>
                            <Editor
                                height={`${inspect.coastfile.split('\n').length * 18 + 16}px`}
                                language="toml"
                                value={inspect.coastfile}
                                beforeMount={handleBeforeMount}
                                theme={activeTheme.id}
                                options={{
                                    readOnly: true,
                                    minimap: { enabled: false },
                                    scrollBeyondLastLine: false,
                                    scrollBeyondLastColumn: 0,
                                    lineNumbers: 'on',
                                    folding: true,
                                    renderLineHighlight: 'none',
                                    overviewRulerBorder: false,
                                    overviewRulerLanes: 0,
                                    hideCursorInOverviewRuler: true,
                                    scrollbar: {
                                        vertical: 'hidden',
                                        horizontal: 'auto',
                                        alwaysConsumeMouseWheel: false,
                                    },
                                    padding: { top: 8, bottom: 0 },
                                    fontSize: 12,
                                    fontFamily:
                                        "'JetBrains Mono', 'Fira Code', Menlo, monospace",
                                    wordWrap: 'off',
                                    domReadOnly: true,
                                    contextmenu: false,
                                }}
                            />
                        </div>
                    )}

                    {inspect.compose != null && (
                        <div className="glass-panel overflow-hidden">
                            <h3 className="text-sm font-semibold text-main px-5 pt-4 pb-2">
                                {t('build.composeOverride')}
                            </h3>
                            <Editor
                                height={`${inspect.compose.split('\n').length * 18 + 16}px`}
                                defaultLanguage="yaml"
                                value={inspect.compose}
                                beforeMount={handleBeforeMount}
                                theme={activeTheme.id}
                                options={{
                                    readOnly: true,
                                    minimap: { enabled: false },
                                    scrollBeyondLastLine: false,
                                    scrollBeyondLastColumn: 0,
                                    lineNumbers: 'on',
                                    folding: true,
                                    renderLineHighlight: 'none',
                                    overviewRulerBorder: false,
                                    overviewRulerLanes: 0,
                                    hideCursorInOverviewRuler: true,
                                    scrollbar: {
                                        vertical: 'hidden',
                                        horizontal: 'auto',
                                        alwaysConsumeMouseWheel: false,
                                    },
                                    padding: { top: 8, bottom: 0 },
                                    fontSize: 12,
                                    fontFamily:
                                        "'JetBrains Mono', 'Fira Code', Menlo, monospace",
                                    wordWrap: 'on',
                                    domReadOnly: true,
                                    contextmenu: false,
                                }}
                            />
                        </div>
                    )}
                </section>
            )}
        </div>
    );
}
