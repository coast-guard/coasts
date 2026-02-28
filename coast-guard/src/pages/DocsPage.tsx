import { useMemo, useState, useCallback, useEffect } from 'react';
import { useLocation, Link, useNavigate } from 'react-router';
import { useTranslation } from 'react-i18next';
import { ArrowLeft, File } from '@phosphor-icons/react';
import { useLocale } from '../hooks/useLocale';
import { api } from '../api/endpoints';
import DocsSidebar from '../components/DocsSidebar';
import DocsViewer from '../components/DocsViewer';
import DocsSearchBar from '../components/DocsSearchBar';
import manifest from '../generated/docs-manifest.json';
import type { TreeNode } from '../components/DocsSidebar';
import type { SearchResult } from '../components/DocsSearchBar';

interface LocaleData {
  tree: TreeNode[];
  files: Record<string, string>;
}

const typedManifest = manifest as { locales: Record<string, LocaleData> };

function getLocaleData(locale: string): LocaleData {
  const data = typedManifest.locales[locale];
  if (data != null) return data;
  return typedManifest.locales['en'] ?? { tree: [], files: {} };
}

function resolveFilePath(urlPath: string, localeData: LocaleData): string | undefined {
  const trimmed = urlPath.replace(/^\/docs\/?/, '');

  if (trimmed === '') {
    if (localeData.files['README.md'] != null) return 'README.md';
    return undefined;
  }

  const withMd = trimmed + '.md';
  if (localeData.files[withMd] != null) return withMd;

  const indexMd = trimmed + '/README.md';
  if (localeData.files[indexMd] != null) return indexMd;

  if (localeData.files[trimmed] != null) return trimmed;

  return undefined;
}

function getBasePath(filePath: string): string {
  const parts = filePath.split('/');
  parts.pop();
  return parts.join('/');
}

export default function DocsPage() {
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();
  const { locale } = useLocale();

  const [searchActive, setSearchActive] = useState(false);
  const [searchResults, setSearchResults] = useState<SearchResult[]>([]);

  const handleSearchActive = useCallback((active: boolean) => setSearchActive(active), []);
  const handleSearchResults = useCallback((results: SearchResult[]) => setSearchResults(results), []);

  const localeData = useMemo(() => getLocaleData(locale), [locale]);
  const enData = useMemo(() => getLocaleData('en'), []);

  const activePath = location.pathname;

  const filePath = useMemo(() => resolveFilePath(activePath, localeData), [activePath, localeData]);

  const content = useMemo(() => {
    if (filePath == null) return undefined;
    const localized = localeData.files[filePath];
    if (localized != null) return localized;
    return enData.files[filePath];
  }, [filePath, localeData, enData]);

  const tree = localeData.tree.length > 0 ? localeData.tree : enData.tree;
  const basePath = filePath != null ? getBasePath(filePath) : '';

  useEffect(() => {
    window.scrollTo(0, 0);
    api.track('docs_page_view', {
      locale,
      doc_path: filePath ?? 'README.md',
      route: activePath,
    });
  }, [locale, filePath, activePath]);

  const handlePanelResultClick = useCallback(
    (result: SearchResult, index: number) => {
      api.track('docs_search_result_open', {
        source: 'results_panel',
        locale,
        selected_index: String(index),
        file_path: result.section.filePath,
        route: result.section.route,
      });
      void navigate(result.section.route);
    },
    [locale, navigate],
  );

  return (
    <div className="page-shell">
      <div className="mb-4">
        <Link
          to="/"
          className="inline-flex items-center gap-1.5 text-sm text-muted-ui hover:text-main transition-colors"
        >
          <ArrowLeft size={14} />
          {t('docs.backToProjects')}
        </Link>
      </div>

      <div className="flex gap-6 items-start">
        <aside className="w-64 flex-shrink-0 glass-panel p-2 pb-[100px] sticky top-20 max-h-[calc(100vh-6rem)] overflow-y-auto">
          <div className="panel-header !px-3 !py-2 !border-0 !bg-transparent">
            {t('docs.title')}
          </div>
          <DocsSidebar tree={tree} activePath={activePath} />
        </aside>

        <div className="flex-1 min-w-0 flex flex-col gap-4">
          <DocsSearchBar
            locale={locale}
            onActiveChange={handleSearchActive}
            onResults={handleSearchResults}
          />

          {searchActive ? (
            <main className="glass-panel p-8">
              {searchResults.length > 0 ? (
                <div className="flex flex-col gap-1">
                  {searchResults.map((r, idx) => (
                    <button
                      key={r.section.id}
                      onClick={() => handlePanelResultClick(r, idx)}
                      className="w-full text-left px-4 py-3 rounded-lg transition-colors hover:bg-white/10 dark:hover:bg-white/5"
                    >
                      <div className="flex items-center gap-2 mb-1">
                        <File size={14} className="flex-shrink-0 text-subtle-ui" />
                        <span className="text-sm font-semibold text-main">{r.section.heading}</span>
                        <span className="text-xs text-subtle-ui ml-auto">{r.section.filePath}</span>
                      </div>
                      <p className="text-sm text-muted-ui line-clamp-2 ml-[22px]">{r.snippet}</p>
                    </button>
                  ))}
                </div>
              ) : (
                <div className="text-center py-12 text-muted-ui">
                  <p className="text-sm">{t('docs.noResults')}</p>
                </div>
              )}
            </main>
          ) : (
            <main className="glass-panel p-8">
              {content != null ? (
                <DocsViewer content={content} basePath={basePath} files={localeData.files} />
              ) : (
                <div className="text-center py-16 text-muted-ui">
                  <p className="text-lg mb-2">{t('docs.notFound')}</p>
                  <p className="text-sm">{t('docs.notFoundHint')}</p>
                </div>
              )}
            </main>
          )}
        </div>
      </div>
    </div>
  );
}
