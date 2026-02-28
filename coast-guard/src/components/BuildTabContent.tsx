import { useCallback, useState } from 'react';
import { useNavigate, Link } from 'react-router';
import { useTranslation } from 'react-i18next';
import Editor, { type BeforeMount } from '@monaco-editor/react';
import { useEditorTheme, ALL_EDITOR_THEMES } from '../hooks/useEditorTheme';
import { setupJsxSupport } from '../lib/monaco-jsx';
import StatusBadge from './StatusBadge';
import type { BuildsInspectResponse, DockerImageInfo, CachedImageInfo } from '../types/api';

function formatBytes(bytes: number): string {
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${Math.round(bytes / 1_048_576)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

function relativeTime(ts: string, t: ReturnType<typeof useTranslation>['t']): string {
  const date = new Date(ts);
  if (isNaN(date.getTime())) return ts;
  const seconds = Math.floor((Date.now() - date.getTime()) / 1000);
  if (seconds < 60) return t('time.justNow');
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return minutes === 1 ? t('time.minuteAgo') : t('time.minutesAgo', { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return hours === 1 ? t('time.hourAgo') : t('time.hoursAgo', { count: hours });
  const days = Math.floor(hours / 24);
  if (days < 30) return days === 1 ? t('time.dayAgo') : t('time.daysAgo', { count: days });
  return t('time.monthsAgo', { count: Math.floor(days / 30) });
}

interface BuildTabContentProps {
  readonly project: string;
  readonly inspect: BuildsInspectResponse | null;
  readonly dockerImages: readonly DockerImageInfo[];
  readonly cachedImages: readonly CachedImageInfo[];
  readonly cachedTotalBytes: number;
  readonly coastfile: string | null;
  readonly compose: string | null;
}

export default function BuildTabContent({
  project,
  inspect,
  dockerImages,
  cachedImages,
  cachedTotalBytes,
  coastfile,
  compose,
}: BuildTabContentProps) {
  const navigate = useNavigate();
  const { t } = useTranslation();
  const { activeTheme } = useEditorTheme();
  const [showTarballs, setShowTarballs] = useState(false);

  const handleBeforeMount: BeforeMount = useCallback((monaco) => {
    setupJsxSupport(monaco, ALL_EDITOR_THEMES);
  }, []);

  if (inspect == null) {
    return (
      <section className="mt-4">
        <div className="glass-panel p-6 text-sm text-subtle-ui">
          {t('build.noBuild')}
        </div>
      </section>
    );
  }

  const coastfileTitle = inspect.coastfile_type != null && inspect.coastfile_type !== 'default'
    ? `${t('build.coastfileResolved')}.${inspect.coastfile_type}`
    : t('build.coastfileResolved');

  return (
    <section className="mt-1 space-y-4">
      <div className="glass-panel p-5">
        <h3 className="text-sm font-semibold text-main mb-3">{t('build.metadata')}</h3>
        <div className="grid grid-cols-2 gap-y-2 gap-x-6 text-sm">
          {inspect.build_id != null && (
            <>
              <span className="text-subtle-ui">{t('build.buildId')}</span>
              <span className="text-main font-mono text-xs break-all">{inspect.build_id}</span>
            </>
          )}
          {inspect.project_root != null && (
            <>
              <span className="text-subtle-ui">{t('build.projectRoot')}</span>
              <span className="text-main font-mono text-xs break-all">{inspect.project_root}</span>
            </>
          )}
          {inspect.build_timestamp != null && (
            <>
              <span className="text-subtle-ui">{t('build.built')}</span>
              <span className="text-main">{relativeTime(inspect.build_timestamp, t)}</span>
            </>
          )}
          <span className="text-subtle-ui">{t('build.type')}</span>
          <span className="text-main">{inspect.coastfile_type ?? 'default'}</span>
          {inspect.coastfile_hash != null && (
            <>
              <span className="text-subtle-ui">{t('build.coastfileHash')}</span>
              <span className="text-main font-mono text-xs">{inspect.coastfile_hash}</span>
            </>
          )}
          {inspect.coast_image != null && (
            <>
              <span className="text-subtle-ui">{t('build.coastImage')}</span>
              <span
                className="font-mono text-xs cursor-pointer text-blue-600 dark:text-blue-400 hover:underline transition-colors"
                onClick={() => navigate(`/project/${project}/host-images/${encodeURIComponent(inspect.coast_image!)}`)}
              >
                {inspect.coast_image}
              </span>
            </>
          )}
          <span className="text-subtle-ui">{t('build.artifact')}</span>
          <span className="text-main font-mono text-xs break-all">
            {inspect.artifact_path} ({formatBytes(inspect.artifact_size_bytes)})
          </span>
        </div>
      </div>

      {inspect.instances.length > 0 && (
        <div className="glass-panel overflow-hidden">
          <h3 className="text-sm font-semibold text-main px-5 pt-4 pb-2">
            {t('build.coastsUsing')} ({inspect.instances.length})
          </h3>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-[var(--border)] text-left text-xs text-subtle-ui">
                  <th className="px-5 py-2 font-medium">{t('col.name')}</th>
                  <th className="px-4 py-2 font-medium">{t('col.status')}</th>
                  <th className="px-4 py-2 font-medium">{t('col.worktree')}</th>
                  <th className="px-4 py-2 font-medium">{t('col.branch')}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--border)]">
                {inspect.instances.map((inst) => (
                  <tr
                    key={String(inst.name)}
                    className="transition-colors cursor-pointer hover:bg-white/35 dark:hover:bg-white/6"
                    onClick={() => navigate(`/instance/${project}/${encodeURIComponent(String(inst.name))}`)}
                  >
                    <td className="px-5 py-2.5">
                      <Link
                        to={`/instance/${project}/${encodeURIComponent(String(inst.name))}`}
                        className="text-blue-600 dark:text-blue-400 hover:underline font-medium"
                        onClick={(e) => e.stopPropagation()}
                      >
                        {String(inst.name)}
                      </Link>
                    </td>
                    <td className="px-4 py-2.5"><StatusBadge status={inst.status} /></td>
                    <td className="px-4 py-2.5 font-mono text-xs">{inst.worktree ?? '\u2014'}</td>
                    <td className="px-4 py-2.5 font-mono text-xs text-subtle-ui">{inst.branch != null ? String(inst.branch) : '\u2014'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}

      <div className="glass-panel overflow-hidden">
        <h3 className="text-sm font-semibold text-main px-5 pt-4 pb-2">
          {t('build.dockerImages')} ({dockerImages.length})
        </h3>
        {dockerImages.length > 0 ? (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-[var(--border)] text-left text-xs text-subtle-ui">
                  <th className="px-5 py-2 font-medium">{t('build.repository')}</th>
                  <th className="px-4 py-2 font-medium">{t('build.tag')}</th>
                  <th className="px-4 py-2 font-medium">{t('build.imageId')}</th>
                  <th className="px-4 py-2 font-medium">{t('build.created')}</th>
                  <th className="px-4 py-2 font-medium">{t('build.size')}</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-[var(--border)]">
                {dockerImages.map((img) => (
                  <tr
                    key={`${img.repository}:${img.tag}`}
                    className="transition-colors cursor-pointer hover:bg-white/35 dark:hover:bg-white/6"
                    onClick={() => navigate(`/project/${project}/host-images/${encodeURIComponent(`${img.repository}:${img.tag}`)}`)}
                  >
                    <td className="px-5 py-2.5 font-mono text-xs text-blue-600 dark:text-blue-400">{img.repository}</td>
                    <td className="px-4 py-2.5">{img.tag}</td>
                    <td className="px-4 py-2.5 font-mono text-xs text-subtle-ui">{img.id}</td>
                    <td className="px-4 py-2.5 text-subtle-ui">{img.created ? relativeTime(img.created, t) : '—'}</td>
                    <td className="px-4 py-2.5">{img.size}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="px-5 pb-4 text-sm text-subtle-ui">{t('build.noDockerImages')}</p>
        )}
      </div>

      {cachedImages.length > 0 && (
        <div className="glass-panel overflow-hidden">
          <button
            type="button"
            className="w-full flex items-center justify-between px-5 py-3 text-sm font-semibold text-main hover:bg-white/20 dark:hover:bg-white/5 transition-colors"
            onClick={() => setShowTarballs(!showTarballs)}
          >
            <span>{t('build.cachedTarballs')} ({cachedImages.length}, {formatBytes(cachedTotalBytes)})</span>
            <span className="text-subtle-ui text-xs">{showTarballs ? '▼' : '▶'}</span>
          </button>
          {showTarballs && (
            <div className="overflow-x-auto border-t border-[var(--border)]">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-[var(--border)] text-left text-xs text-subtle-ui">
                    <th className="px-5 py-2 font-medium">{t('build.type')}</th>
                    <th className="px-4 py-2 font-medium">{t('build.reference')}</th>
                    <th className="px-4 py-2 font-medium">{t('build.size')}</th>
                    <th className="px-4 py-2 font-medium">{t('build.cached')}</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[var(--border)]">
                  {cachedImages.map((img) => (
                    <tr key={img.filename}>
                      <td className="px-5 py-2.5">
                        <span className={`inline-block px-1.5 py-0.5 rounded text-[10px] font-medium ${
                          img.image_type === 'built'
                            ? 'bg-blue-500/15 text-blue-700 dark:text-blue-300'
                            : img.image_type === 'base'
                              ? 'bg-slate-500/15 text-slate-600 dark:text-slate-400'
                              : 'bg-green-500/15 text-green-700 dark:text-green-300'
                        }`}>
                          {img.image_type}
                        </span>
                      </td>
                      <td className="px-4 py-2.5 font-mono text-xs">{img.reference}</td>
                      <td className="px-4 py-2.5">{formatBytes(img.size_bytes)}</td>
                      <td className="px-4 py-2.5 text-subtle-ui">
                        {img.modified != null ? relativeTime(img.modified, t) : '—'}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {inspect.shared_services && inspect.shared_services.length > 0 && (
        <div className="glass-panel p-5">
          <h3 className="text-sm font-semibold text-main mb-2">{t('build.sharedServices')} ({inspect.shared_services.length})</h3>
          <div className="flex flex-wrap gap-1.5">
            {inspect.shared_services.map((s) => (
              <span
                key={s.name}
                className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-mono bg-teal-500/15 text-teal-700 dark:text-teal-300"
              >
                {s.name}
                <span className="text-[10px] opacity-70">{s.image}</span>
                {s.ports.length > 0 && <span className="text-[10px] opacity-60">:{s.ports.join(',')}</span>}
              </span>
            ))}
          </div>
        </div>
      )}

      {inspect.volumes && inspect.volumes.length > 0 && (
        <div className="glass-panel p-5">
          <h3 className="text-sm font-semibold text-main mb-2">{t('build.volumeStrategies')} ({inspect.volumes.length})</h3>
          <div className="flex flex-wrap gap-1.5">
            {inspect.volumes.map((v) => (
              <span
                key={v.name}
                className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-mono ${
                  v.strategy === 'shared'
                    ? 'bg-cyan-500/15 text-cyan-700 dark:text-cyan-300'
                    : 'bg-slate-500/15 text-slate-600 dark:text-slate-400'
                }`}
              >
                {v.name}
                <span className="text-[10px] opacity-70">{v.strategy}</span>
                <span className="text-[10px] opacity-60">{v.service}:{v.mount}</span>
              </span>
            ))}
          </div>
        </div>
      )}

      {inspect.secrets.length > 0 && (
        <div className="glass-panel p-5">
          <h3 className="text-sm font-semibold text-main mb-2">{t('build.secrets')} ({inspect.secrets.length})</h3>
          <div className="flex flex-wrap gap-1.5">
            {inspect.secrets.map((s) => (
              <span key={s} className="inline-block px-2 py-0.5 rounded text-xs font-mono bg-amber-500/15 text-amber-700 dark:text-amber-300">
                {s}
              </span>
            ))}
          </div>
        </div>
      )}

      {((inspect.mcp_servers && inspect.mcp_servers.length > 0) || (inspect.mcp_clients && inspect.mcp_clients.length > 0)) && (
        <div className="glass-panel p-5 space-y-3">
          {inspect.mcp_servers && inspect.mcp_servers.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold text-main mb-2">{t('build.mcpServers')} ({inspect.mcp_servers.length})</h3>
              <div className="flex flex-wrap gap-1.5">
                {inspect.mcp_servers.map((m) => (
                  <span
                    key={m.name}
                    className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-mono ${
                      m.proxy
                        ? 'bg-yellow-500/15 text-yellow-700 dark:text-yellow-300'
                        : 'bg-blue-500/15 text-blue-700 dark:text-blue-300'
                    }`}
                  >
                    {m.name}
                    <span className="text-[10px] opacity-70">{m.proxy ? 'host' : 'internal'}</span>
                  </span>
                ))}
              </div>
            </div>
          )}
          {inspect.mcp_clients && inspect.mcp_clients.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold text-main mb-2">{t('build.mcpClients')} ({inspect.mcp_clients.length})</h3>
              <div className="flex flex-wrap gap-1.5">
                {inspect.mcp_clients.map((c) => (
                  <span key={c.name} className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-mono bg-purple-500/15 text-purple-700 dark:text-purple-300">
                    {c.name}
                    {c.config_path && <span className="text-[10px] opacity-70">{c.config_path}</span>}
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {(inspect.omitted_services.length > 0 || inspect.omitted_volumes.length > 0) && (
        <div className="glass-panel p-5 space-y-3">
          {inspect.omitted_services.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold text-main mb-2">{t('build.omittedServices')}</h3>
              <div className="flex flex-wrap gap-1.5">
                {inspect.omitted_services.map((s) => (
                  <span key={s} className="inline-block px-2 py-0.5 rounded text-xs font-mono bg-rose-500/15 text-rose-700 dark:text-rose-300">
                    {s}
                  </span>
                ))}
              </div>
            </div>
          )}
          {inspect.omitted_volumes.length > 0 && (
            <div>
              <h3 className="text-sm font-semibold text-main mb-2">{t('build.omittedVolumes')}</h3>
              <div className="flex flex-wrap gap-1.5">
                {inspect.omitted_volumes.map((v) => (
                  <span key={v} className="inline-block px-2 py-0.5 rounded text-xs font-mono bg-rose-500/15 text-rose-700 dark:text-rose-300">
                    {v}
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>
      )}

      {coastfile != null && (
        <div className="glass-panel overflow-hidden">
          <h3 className="text-sm font-semibold text-main px-5 pt-4 pb-2">{coastfileTitle}</h3>
          <Editor
            height={`${coastfile.split('\n').length * 18 + 16}px`}
            language="toml"
            value={coastfile}
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
              scrollbar: { vertical: 'hidden', horizontal: 'auto', alwaysConsumeMouseWheel: false },
              padding: { top: 8, bottom: 0 },
              fontSize: 12,
              fontFamily: "'JetBrains Mono', 'Fira Code', Menlo, monospace",
              wordWrap: 'off',
              domReadOnly: true,
              contextmenu: false,
            }}
          />
        </div>
      )}

      {compose != null && (
        <div className="glass-panel overflow-hidden">
          <h3 className="text-sm font-semibold text-main px-5 pt-4 pb-2">{t('build.composeOverride')}</h3>
          <Editor
            height={`${compose.split('\n').length * 18 + 16}px`}
            defaultLanguage="yaml"
            value={compose}
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
              scrollbar: { vertical: 'hidden', horizontal: 'auto', alwaysConsumeMouseWheel: false },
              padding: { top: 8, bottom: 0 },
              fontSize: 12,
              fontFamily: "'JetBrains Mono', 'Fira Code', Menlo, monospace",
              wordWrap: 'on',
              domReadOnly: true,
              contextmenu: false,
            }}
          />
        </div>
      )}
    </section>
  );
}
