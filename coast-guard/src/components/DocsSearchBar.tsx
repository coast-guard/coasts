import { useState, useRef, useCallback } from 'react';
import { useNavigate } from 'react-router';
import { useTranslation } from 'react-i18next';
import { MagnifyingGlass, X, File } from '@phosphor-icons/react';
import { api } from '../api/endpoints';
import type { DocsSearchResult } from '../api/endpoints';

export interface SearchResult {
  section: {
    id: string;
    filePath: string;
    heading: string;
    route: string;
  };
  snippet: string;
  score: number;
}

function toSearchResult(r: DocsSearchResult, idx: number): SearchResult {
  return {
    section: {
      id: `${idx}-${r.path}`,
      filePath: r.path,
      heading: r.heading,
      route: r.route,
    },
    snippet: r.snippet,
    score: r.score,
  };
}

interface DocsSearchBarProps {
  locale: string;
  onActiveChange: (active: boolean) => void;
  onResults: (results: SearchResult[]) => void;
}

export default function DocsSearchBar({ locale, onActiveChange, onResults }: DocsSearchBarProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [selectedIdx, setSelectedIdx] = useState(-1);
  const [searching, setSearching] = useState(false);
  const [searchDone, setSearchDone] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const requestIdRef = useRef(0);

  const doSearch = useCallback(
    (q: string) => {
      if (q.trim().length === 0) {
        setResults([]);
        onResults([]);
        onActiveChange(false);
        setSearchDone(false);
        return;
      }
      api.track('docs_search_submitted', { locale, query: q });

      const requestId = ++requestIdRef.current;
      setSearching(true);
      setSearchDone(false);

      void api.docsSearch(q, locale).then((resp) => {
        if (requestIdRef.current !== requestId) return;
        const mapped = resp.results.map(toSearchResult);
        setResults(mapped);
        onResults(mapped);
        onActiveChange(true);
        setSearching(false);
        setSearchDone(true);
        api.track('docs_search_results', {
          locale,
          query: q,
          result_count: String(mapped.length),
        });
        setSelectedIdx(-1);
      }).catch(() => {
        if (requestIdRef.current !== requestId) return;
        setSearching(false);
        setSearchDone(true);
      });
    },
    [locale, onResults, onActiveChange],
  );

  const handleChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const val = e.target.value;
      setQuery(val);
      if (debounceRef.current != null) clearTimeout(debounceRef.current);
      if (val.trim().length === 0) {
        setResults([]);
        onResults([]);
        onActiveChange(false);
        setSearchDone(false);
        return;
      }
      debounceRef.current = setTimeout(() => doSearch(val), 200);
    },
    [doSearch, onResults, onActiveChange],
  );

  const handleClear = useCallback(() => {
    setQuery('');
    setResults([]);
    onResults([]);
    onActiveChange(false);
    setSearchDone(false);
    inputRef.current?.focus();
  }, [onResults, onActiveChange]);

  const navigateToResult = useCallback(
    (
      result: SearchResult,
      selectedIndex: number | undefined,
      source: 'search_dropdown' | 'keyboard',
    ) => {
      api.track('docs_search_result_open', {
        source,
        locale,
        query,
        selected_index: String(selectedIndex ?? -1),
        file_path: result.section.filePath,
        route: result.section.route,
      });
      setQuery('');
      setResults([]);
      onResults([]);
      onActiveChange(false);
      setSearchDone(false);
      void navigate(result.section.route);
    },
    [locale, navigate, onActiveChange, onResults, query],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Escape') {
        handleClear();
        return;
      }
      if (results.length === 0) return;

      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedIdx((prev) => (prev < results.length - 1 ? prev + 1 : 0));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedIdx((prev) => (prev > 0 ? prev - 1 : results.length - 1));
      } else if (e.key === 'Enter' && selectedIdx >= 0) {
        e.preventDefault();
        const selected = results[selectedIdx];
        if (selected != null) navigateToResult(selected, selectedIdx, 'keyboard');
      }
    },
    [results, selectedIdx, handleClear, navigateToResult],
  );

  const hasQuery = query.trim().length > 0;

  return (
    <div className="relative">
      <div className="relative">
        <MagnifyingGlass
          size={16}
          className="absolute left-3 top-1/2 -translate-y-1/2 text-subtle-ui pointer-events-none"
        />
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={handleChange}
          onKeyDown={handleKeyDown}
          placeholder={t('docs.searchPlaceholder')}
          className="w-full h-10 pl-9 pr-9 rounded-lg bg-[var(--surface)] border border-[var(--border)] text-main text-sm placeholder:text-subtle-ui focus:outline-none focus:ring-2 focus:ring-[var(--focus-ring)] transition-colors"
        />
        {hasQuery && (
          <button
            onClick={handleClear}
            className="absolute right-2 top-1/2 -translate-y-1/2 w-6 h-6 inline-flex items-center justify-center rounded text-subtle-ui hover:text-main hover:bg-white/10 transition-colors"
          >
            <X size={14} />
          </button>
        )}
      </div>

      {hasQuery && results.length > 0 && (
        <div className="absolute z-50 top-full left-0 right-0 mt-2 glass-panel py-1 max-h-80 overflow-y-auto">
          <div className="px-3 py-1.5 text-xs text-subtle-ui">
            {t('docs.resultCount', { count: results.length })}
          </div>
          {results.map((r, i) => (
            <button
              key={r.section.id}
              onClick={() => navigateToResult(r, i, 'search_dropdown')}
              onMouseEnter={() => setSelectedIdx(i)}
              className={`w-full text-left px-3 py-2.5 transition-colors ${
                i === selectedIdx
                  ? 'bg-white/20 dark:bg-white/10'
                  : 'hover:bg-white/10 dark:hover:bg-white/5'
              }`}
            >
              <div className="flex items-center gap-2 mb-0.5">
                <File size={14} className="flex-shrink-0 text-subtle-ui" />
                <span className="text-sm font-medium text-main truncate">
                  {r.section.heading}
                </span>
              </div>
              <p className="text-xs text-muted-ui line-clamp-2 ml-[22px]">
                {r.snippet}
              </p>
            </button>
          ))}
        </div>
      )}

      {hasQuery && results.length === 0 && !searching && searchDone && (
        <div className="absolute z-50 top-full left-0 right-0 mt-2 glass-panel py-4 text-center text-sm text-muted-ui">
          {t('docs.noResults')}
        </div>
      )}
    </div>
  );
}
