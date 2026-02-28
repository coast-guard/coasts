import { useState, useEffect, useCallback, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import Editor, { type OnMount, type BeforeMount, type Monaco } from '@monaco-editor/react';
import { FloppyDisk, CornersOut, CornersIn, MagnifyingGlass, Circle, X, SidebarSimple, Asterisk, TextT } from '@phosphor-icons/react';
import type { ProjectName, InstanceName } from '../types/branded';
import { api } from '../api/endpoints';
import FileTree, { type FileTreeHandle } from '../components/FileTree';
import { LspClient } from '../lib/lsp-client';
import { createLspBridge, extToLspLanguage, lspConnectionLanguage, type LspBridge } from '../lib/monaco-lsp-bridge';
import { setupJsxSupport } from '../lib/monaco-jsx';
import { useEditorTheme, ALL_EDITOR_THEMES } from '../hooks/useEditorTheme';
import EditorThemePicker from '../components/EditorThemePicker';

interface Props {
  readonly project: ProjectName;
  readonly name: InstanceName;
}

function extToLanguage(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  const map: Record<string, string> = {
    ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript',
    mjs: 'javascript', cjs: 'javascript', json: 'json', html: 'html',
    htm: 'html', css: 'css', scss: 'scss', less: 'less', md: 'markdown',
    yaml: 'yaml', yml: 'yaml', toml: 'ini', xml: 'xml', sql: 'sql',
    sh: 'shell', bash: 'shell', zsh: 'shell', py: 'python', rs: 'rust',
    go: 'go', java: 'java', rb: 'ruby', php: 'php', c: 'c', cpp: 'cpp',
    h: 'c', hpp: 'cpp', cs: 'csharp', swift: 'swift', kt: 'kotlin',
    dockerfile: 'dockerfile', makefile: 'makefile', graphql: 'graphql',
    proto: 'protobuf', env: 'ini', gitignore: 'ini', dockerignore: 'ini',
  };
  const fname = path.split('/').pop()?.toLowerCase() ?? '';
  if (fname === 'dockerfile' || fname.startsWith('dockerfile.')) return 'dockerfile';
  if (fname === 'makefile') return 'makefile';
  return map[ext] ?? 'plaintext';
}

function basename(path: string): string {
  const slash = path.lastIndexOf('/');
  return slash >= 0 ? path.slice(slash + 1) : path;
}

function dirname(path: string): string {
  const slash = path.lastIndexOf('/');
  return slash >= 0 ? path.slice(0, slash) : '';
}

interface IndexEntry {
  readonly path: string;
  readonly lower: string;
  readonly basename: string;
}

function fuzzyScore(query: string, entry: IndexEntry): number {
  const lq = query.toLowerCase();
  const lp = entry.lower;
  if (lq.length === 0) return 0;
  let qi = 0;
  let score = 0;
  let prevMatch = false;
  let firstMatchInBasename = false;
  for (let pi = 0; pi < lp.length && qi < lq.length; pi++) {
    if (lp[pi] === lq[qi]) {
      qi++;
      score += prevMatch ? 10 : 1;
      const c = lp[pi - 1];
      if (pi === 0 || c === '/' || c === '.' || c === '-' || c === '_') score += 5;
      if (!firstMatchInBasename && pi >= lp.length - entry.basename.length) firstMatchInBasename = true;
      prevMatch = true;
    } else {
      prevMatch = false;
    }
  }
  if (qi < lq.length) return -1;
  if (firstMatchInBasename) score += 20;
  if (entry.basename.toLowerCase().startsWith(lq)) score += 50;
  return score;
}

function buildIndex(paths: readonly string[]): IndexEntry[] {
  return paths.map((p) => {
    const slash = p.lastIndexOf('/');
    return { path: p, lower: p.toLowerCase(), basename: slash >= 0 ? p.slice(slash + 1) : p };
  });
}

interface GrepResult { readonly path: string; readonly line: number; readonly text: string; }
interface FuzzyResult { readonly path: string; readonly basename: string; readonly dir: string; readonly score: number; }

interface TabContent { content: string; saved: string; }

// Preserved cursor/scroll positions keyed by Monaco URI (e.g. "file:///workspace/src/foo.ts").
// Module-level so they survive component remounts.
const editorViewStates = new Map<string, unknown>();

// Monaco's setModel/dispose cancels in-flight Delayer/Throttler promises which reject
// with CancellationError. Nothing inside Monaco catches them. Install once, never remove.
let _monacoHandlerInstalled = false;
function installMonacoCancelHandler() {
  if (_monacoHandlerInstalled) return;
  _monacoHandlerInstalled = true;
  window.addEventListener('unhandledrejection', (e) => {
    if (e.reason?.name === 'Canceled' && e.reason?.message === 'Canceled') e.preventDefault();
  });
}

/**
 * Switch the Monaco editor to a different file model, suppressing the
 * CancellationError rejections that Monaco's internal Delayer/Throttler
 * produces when in-flight operations are cancelled by `setModel`.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function switchEditorModel(editor: any, monaco: Monaco, uri: string, fileContent: string, lang: string) {
  // Guard against calling setModel on a disposed editor (stale ref after unmount)
  try { editor.getDomNode(); } catch { return null; }
  if (editor.getDomNode() == null) return null;

  const currentModel = editor.getModel();
  if (currentModel != null) {
    const state = editor.saveViewState();
    if (state != null) editorViewStates.set(currentModel.uri.toString(), state);
  }

  const parsedUri = monaco.Uri.parse(uri);
  let model = monaco.editor.getModel(parsedUri);
  if (model == null) {
    model = monaco.editor.createModel(fileContent, lang, parsedUri);
  } else {
    if (model.getValue() !== fileContent) model.setValue(fileContent);
    if (model.getLanguageId() !== lang) monaco.editor.setModelLanguage(model, lang);
  }

  if (model !== currentModel) {
    editor.setModel(model);
  }

  const saved = editorViewStates.get(uri);
  if (saved != null) editor.restoreViewState(saved);

  return model;
}

// --- Persistence helper ---
function usePersist(key: string, value: string | null, debounceMs: number) {
  const timerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const prevRef = useRef<string | null>(null);
  useEffect(() => {
    if (value === prevRef.current) return;
    prevRef.current = value;
    if (value == null) return;
    clearTimeout(timerRef.current);
    if (debounceMs <= 0) {
      void api.setSetting(key, value);
    } else {
      timerRef.current = setTimeout(() => { void api.setSetting(key, value); }, debounceMs);
    }
    return () => clearTimeout(timerRef.current);
  }, [key, value, debounceMs]);
}

export default function InstanceFilesTab({ project, name }: Props) {
  const { t } = useTranslation();
  const p = project as string;
  const n = name as string;
  const { activeTheme, themes: editorThemes, setEditorTheme } = useEditorTheme();

  // --- Tab state ---
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [tabStack, setTabStack] = useState<string[]>([]);
  const [activePath, setActivePath] = useState<string | null>(null);
  const tabContentsRef = useRef<Map<string, TabContent>>(new Map());

  const [content, setContent] = useState<string>('');
  const [savedContent, setSavedContent] = useState<string>('');
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [expandedPaths, setExpandedPaths] = useState<string[]>([]);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const editorRef = useRef<any>(null);
  const fileTreeRef = useRef<FileTreeHandle>(null);

  // File index for fuzzy search
  const indexRef = useRef<IndexEntry[]>([]);
  const [indexCount, setIndexCount] = useState(0);
  const [indexLoading, setIndexLoading] = useState(true);

  // Ctrl+P fuzzy file search
  const [showFileSearch, setShowFileSearch] = useState(false);
  const [fileSearchQuery, setFileSearchQuery] = useState('');
  const [fileSearchResults, setFileSearchResults] = useState<FuzzyResult[]>([]);
  const [selectedIdx, setSelectedIdx] = useState(0);
  const fileSearchRef = useRef<HTMLInputElement>(null);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Ctrl+Shift+F content search
  const [showGrepPanel, setShowGrepPanel] = useState(false);
  const [grepQuery, setGrepQuery] = useState('');
  const [grepRegex, setGrepRegex] = useState(false);
  const [grepResults, setGrepResults] = useState<readonly GrepResult[]>([]);
  const [grepSearching, setGrepSearching] = useState(false);
  const grepInputRef = useRef<HTMLInputElement>(null);
  const grepTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Git status
  const [gitStatus, setGitStatus] = useState<Map<string, string>>(new Map());

  // LSP state
  const lspClientRef = useRef<LspClient | null>(null);
  const lspBridgeRef = useRef<LspBridge | null>(null);
  const lspLanguageRef = useRef<string | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const [lspConnected, setLspConnected] = useState(false);
  const [monacoReady, setMonacoReady] = useState(false);
  const docVersionRef = useRef(0);
  const lspChangeTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Restoration flag
  const [restored, setRestored] = useState(false);

  // Stable initial path for <Editor> — captured once so the path prop never changes,
  // preventing @monaco-editor/react from calling setModel internally.
  const initialEditorPathRef = useRef<string | null>(null);

  const isDirty = content !== savedContent && activePath != null;

  // --- Persistence ---
  usePersist(`files:${p}:${n}:tabs`, restored ? JSON.stringify(openTabs) : null, 300);
  usePersist(`files:${p}:${n}:activeTab`, restored ? (activePath ?? '') : null, 300);
  usePersist(`files:${p}:${n}:tabStack`, restored ? JSON.stringify(tabStack) : null, 300);
  usePersist(`files:${p}:${n}:sidebar`, restored ? (sidebarOpen ? 'open' : 'closed') : null, 0);
  usePersist(`files:${p}:${n}:grepQuery`, restored ? grepQuery : null, 500);
  usePersist(`files:${p}:${n}:grepRegex`, restored ? String(grepRegex) : null, 0);
  usePersist(`files:${p}:${n}:grepOpen`, restored ? String(showGrepPanel) : null, 0);
  usePersist(`files:${p}:${n}:fullscreen`, restored ? String(fullscreen) : null, 0);
  usePersist(`files:${p}:${n}:expanded`, restored ? JSON.stringify(expandedPaths) : null, 300);

  // --- Restore persisted state on mount ---
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const keys = ['tabs', 'activeTab', 'tabStack', 'sidebar', 'grepQuery', 'grepRegex', 'grepOpen', 'fullscreen', 'expanded'];
        const results = await Promise.all(keys.map((k) => api.getSetting(`files:${p}:${n}:${k}`)));
        if (cancelled) return;
        const [tabsRaw, activeRaw, stackRaw, sidebarRaw, grepQRaw, grepRRaw, grepORaw, fsRaw, expandedRaw] = results;
        if (tabsRaw != null) try { setOpenTabs(JSON.parse(tabsRaw)); } catch { /* ignore */ }
        if (stackRaw != null) try { setTabStack(JSON.parse(stackRaw)); } catch { /* ignore */ }
        if (sidebarRaw != null) setSidebarOpen(sidebarRaw !== 'closed');
        if (grepQRaw != null) setGrepQuery(grepQRaw);
        if (grepRRaw != null) setGrepRegex(grepRRaw === 'true');
        if (grepORaw != null) setShowGrepPanel(grepORaw === 'true');
        if (fsRaw != null) setFullscreen(fsRaw === 'true');
        if (expandedRaw != null) try { setExpandedPaths(JSON.parse(expandedRaw)); } catch { /* ignore */ }
        // Restore active file last (triggers file load)
        if (activeRaw != null && activeRaw.length > 0) {
          setActivePath(activeRaw);
        }
      } catch { /* ignore */ }
      if (!cancelled) setRestored(true);
    })();
    return () => { cancelled = true; };
  }, [p, n]);

  // Load restored file content after restore
  useEffect(() => {
    if (!restored || activePath == null) return;
    if (tabContentsRef.current.has(activePath)) return;
    void (async () => {
      setLoading(true);
      setError(null);
      try {
        const data = await api.fileRead(p, n, activePath);
        setContent(data.content);
        setSavedContent(data.content);
        tabContentsRef.current.set(activePath, { content: data.content, saved: data.content });
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    })();
  }, [restored, activePath, p, n]);

  // Load file index
  useEffect(() => {
    let cancelled = false;
    setIndexLoading(true);
    void (async () => {
      try {
        const paths = await api.fileIndex(p, n);
        if (cancelled) return;
        indexRef.current = buildIndex(paths);
        setIndexCount(paths.length);
      } catch { /* ignore */ }
      if (!cancelled) setIndexLoading(false);
    })();
    return () => { cancelled = true; };
  }, [p, n]);

  // Load git status
  const refreshGitStatus = useCallback(async () => {
    try {
      const statuses = await api.fileGitStatus(p, n);
      const map = new Map<string, string>();
      for (const s of statuses) map.set(s.path, s.status);
      setGitStatus(map);
    } catch { /* ignore */ }
  }, [p, n]);

  useEffect(() => {
    void refreshGitStatus();
    const iv = setInterval(() => void refreshGitStatus(), 30000);
    return () => clearInterval(iv);
  }, [refreshGitStatus]);

  // --- Tab helpers ---
  const saveCurrentTabContent = useCallback(() => {
    if (activePath != null) {
      tabContentsRef.current.set(activePath, { content, saved: savedContent });
    }
  }, [activePath, content, savedContent]);

  const switchToTab = useCallback((path: string) => {
    if (activePath != null && activePath !== path && lspClientRef.current?.isReady) {
      lspClientRef.current.didClose(`file://${activePath}`);
    }
    saveCurrentTabContent();
    setActivePath(path);
    const cached = tabContentsRef.current.get(path);
    if (cached != null) {
      setContent(cached.content);
      setSavedContent(cached.saved);
      setLoading(false);
      setError(null);
      const lang = extToLspLanguage(path);
      if (lang != null && lspClientRef.current?.isReady) {
        docVersionRef.current++;
        lspClientRef.current.didOpen(`file://${path}`, lang, docVersionRef.current, cached.content);
      }
      const ed = editorRef.current;
      const m = monacoRef.current;
      if (ed != null && m != null) {
        const model = switchEditorModel(ed, m, `file://${path}`, cached.content, extToLanguage(path));
        if (lspBridgeRef.current != null) lspBridgeRef.current.setModel(model, `file://${path}`);
      }
    }
    setTabStack((prev) => [path, ...prev.filter((p) => p !== path)]);
    fileTreeRef.current?.revealPath(path);
  }, [activePath, saveCurrentTabContent]);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const openFileRef = useRef<(path: string, line?: number) => void>(null as any);

  const openFileInTab = useCallback(
    async (path: string, line?: number) => {
      const fullPath = path.startsWith('/workspace') ? path : `/workspace/${path}`;

      // LSP: close previous document
      if (activePath != null && lspClientRef.current?.isReady) {
        lspClientRef.current.didClose(`file://${activePath}`);
      }

      saveCurrentTabContent();

      // Add to tabs if not present
      setOpenTabs((prev) => prev.includes(fullPath) ? prev : [...prev, fullPath]);
      setTabStack((prev) => [fullPath, ...prev.filter((p) => p !== fullPath)]);
      setActivePath(fullPath);

      // Reveal in tree
      fileTreeRef.current?.revealPath(fullPath);

      // Check content cache
      const cached = tabContentsRef.current.get(fullPath);
      if (cached != null) {
        setContent(cached.content);
        setSavedContent(cached.saved);
        setLoading(false);
        setError(null);
        // LSP: open
        const lang = extToLspLanguage(fullPath);
        if (lang != null && lspClientRef.current?.isReady) {
          docVersionRef.current++;
          lspClientRef.current.didOpen(`file://${fullPath}`, lang, docVersionRef.current, cached.content);
        }
        // Switch model eagerly so the editor shows the file before React re-renders
        const ed = editorRef.current;
        const m = monacoRef.current;
        if (ed != null && m != null) {
          const model = switchEditorModel(ed, m, `file://${fullPath}`, cached.content, extToLanguage(fullPath));
          if (lspBridgeRef.current != null) lspBridgeRef.current.setModel(model, `file://${fullPath}`);
        }
        if (line != null) {
          setTimeout(() => {
            editorRef.current?.revealLineInCenter(line);
            editorRef.current?.setPosition({ lineNumber: line, column: 1 });
          }, 50);
        }
        return;
      }

      setLoading(true);
      setError(null);
      try {
        const data = await api.fileRead(p, n, fullPath);
        setContent(data.content);
        setSavedContent(data.content);
        tabContentsRef.current.set(fullPath, { content: data.content, saved: data.content });
        // LSP: open
        const lang = extToLspLanguage(fullPath);
        if (lang != null && lspClientRef.current?.isReady) {
          docVersionRef.current++;
          lspClientRef.current.didOpen(`file://${fullPath}`, lang, docVersionRef.current, data.content);
        }
        // Switch model after content is loaded
        const ed = editorRef.current;
        const m = monacoRef.current;
        if (ed != null && m != null) {
          const model = switchEditorModel(ed, m, `file://${fullPath}`, data.content, extToLanguage(fullPath));
          if (lspBridgeRef.current != null) lspBridgeRef.current.setModel(model, `file://${fullPath}`);
        }
        if (line != null) {
          setTimeout(() => {
            editorRef.current?.revealLineInCenter(line);
            editorRef.current?.setPosition({ lineNumber: line, column: 1 });
          }, 200);
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setLoading(false);
      }
    },
    [p, n, activePath, saveCurrentTabContent],
  );
  openFileRef.current = (path: string, line?: number) => void openFileInTab(path, line);

  const closeTab = useCallback((path: string) => {
    tabContentsRef.current.delete(path);
    setOpenTabs((prev) => prev.filter((t) => t !== path));
    setTabStack((prev) => prev.filter((t) => t !== path));
    if (activePath === path) {
      // Switch to next MRU tab
      const remaining = tabStack.filter((t) => t !== path);
      if (remaining.length > 0) {
        switchToTab(remaining[0]!);
      } else {
        setActivePath(null);
        setContent('');
        setSavedContent('');
      }
    }
  }, [activePath, tabStack, switchToTab]);

  const handleSave = useCallback(async () => {
    if (activePath == null || !isDirty) return;
    setSaving(true);
    try {
      await api.fileWrite(p, n, activePath, content);
      setSavedContent(content);
      tabContentsRef.current.set(activePath, { content, saved: content });
      void refreshGitStatus();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  }, [p, n, activePath, content, isDirty, refreshGitStatus]);

  // --- LSP ---
  // Find nearest project root by checking for config files in the file index
  const findProjectRoot = useCallback((filePath: string): string | undefined => {
    const configFiles = ['tsconfig.json', 'package.json', 'Cargo.toml', 'go.mod', 'pyproject.toml'];
    const index = indexRef.current;
    if (index.length === 0) return undefined;
    const indexSet = new Set(index.map((e) => e.path));
    const relative = filePath.replace('/workspace/', '');
    const parts = relative.split('/');
    // Walk up from the file's directory, checking each ancestor for config files
    for (let i = parts.length - 1; i >= 1; i--) {
      const dir = parts.slice(0, i).join('/');
      for (const cf of configFiles) {
        if (indexSet.has(`${dir}/${cf}`)) {
          return `/workspace/${dir}`;
        }
      }
    }
    return undefined;
  }, []);

  const lspRootRef = useRef<string | undefined>(undefined);

  const activePathRef = useRef(activePath);
  activePathRef.current = activePath;
  const contentRef = useRef(content);
  contentRef.current = content;

  const connectLsp = useCallback(
    (lspLang: string, filePath: string | null) => {
      const projectRoot = filePath != null ? findProjectRoot(filePath) : undefined;
      const rootKey = `${lspLang}:${projectRoot ?? '/workspace'}`;
      const prevKey = `${lspLanguageRef.current ?? ''}:${lspRootRef.current ?? '/workspace'}`;
      if (rootKey === prevKey && lspClientRef.current != null) return;

      lspBridgeRef.current?.dispose();
      lspBridgeRef.current = null;
      lspClientRef.current?.dispose();
      lspClientRef.current = null;
      lspLanguageRef.current = null;
      lspRootRef.current = projectRoot;
      setLspConnected(false);
      const connLang = lspConnectionLanguage(lspLang);
      const m = monacoRef.current;
      if (m == null) return;
      const rootUri = projectRoot != null ? `file://${projectRoot}` : 'file:///workspace';
      const client = new LspClient({
        project: p, name: n, language: connLang, rootUri, rootPath: projectRoot,
        onServerReady: () => {
          setLspConnected(true);
          const ap = activePathRef.current;
          if (ap != null) {
            const lang = extToLspLanguage(ap);
            if (lang != null) { docVersionRef.current++; client.didOpen(`file://${ap}`, lang, docVersionRef.current, contentRef.current); }
          }
        },
        onError: () => { /* LSP error – no user-facing action */ },
        onClose: () => setLspConnected(false),
      });
      const bridge = createLspBridge({
        monaco: m, languageId: lspLang, client,
        onOpenFile: (path, line) => openFileRef.current?.(path, line),
      });
      const editor = editorRef.current;
      if (editor != null) {
        bridge.setEditor(editor);
        const model = editor.getModel();
        const ap = activePathRef.current;
        if (model != null) bridge.setModel(model, ap != null ? `file://${ap}` : undefined);
      }
      lspClientRef.current = client;
      lspBridgeRef.current = bridge;
      lspLanguageRef.current = lspLang;
      client.connect();
    },
    [p, n, findProjectRoot],
  );

  useEffect(() => {
    if (!monacoReady || activePath == null) return;
    const lspLang = extToLspLanguage(activePath);
    if (lspLang == null) {
      if (lspClientRef.current != null) {
        lspBridgeRef.current?.dispose(); lspBridgeRef.current = null;
        lspClientRef.current.dispose(); lspClientRef.current = null;
        lspLanguageRef.current = null; setLspConnected(false);
      }
      return;
    }
    connectLsp(lspLang, activePath);
  }, [activePath, connectLsp, monacoReady, indexLoading]);

  useEffect(() => {
    if (activePath == null || lspClientRef.current == null || !lspConnected) return;
    clearTimeout(lspChangeTimerRef.current);
    lspChangeTimerRef.current = setTimeout(() => {
      const client = lspClientRef.current;
      if (client == null || !client.isReady) return;
      docVersionRef.current++;
      client.didChange(`file://${activePath}`, docVersionRef.current, content);
    }, 50);
    return () => clearTimeout(lspChangeTimerRef.current);
  }, [content, activePath, lspConnected]);

  useEffect(() => {
    return () => { lspBridgeRef.current?.dispose(); lspClientRef.current?.dispose(); };
  }, []);

  // --- Keyboard shortcuts (capture phase) ---
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault(); e.stopPropagation(); void handleSave();
      }
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && e.key === 'p') {
        e.preventDefault(); e.stopPropagation();
        setShowFileSearch(true); setShowGrepPanel(false);
      }
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === 'f' || e.key === 'F')) {
        e.preventDefault(); e.stopPropagation();
        setShowGrepPanel((v) => !v); setShowFileSearch(false);
      }
      if (e.key === 'Escape') {
        if (showFileSearch) { setShowFileSearch(false); }
      }
    }
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [handleSave, showFileSearch]);

  // Focus file search input
  useEffect(() => {
    if (showFileSearch) setTimeout(() => fileSearchRef.current?.focus(), 30);
    else { setFileSearchQuery(''); setFileSearchResults([]); setSelectedIdx(0); }
  }, [showFileSearch]);

  useEffect(() => {
    if (showGrepPanel) setTimeout(() => grepInputRef.current?.focus(), 30);
  }, [showGrepPanel]);

  // Fuzzy search
  useEffect(() => {
    if (!showFileSearch || fileSearchQuery.length < 1) {
      setFileSearchResults([]); setSelectedIdx(0); return;
    }
    clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      const index = indexRef.current;
      const scored: FuzzyResult[] = [];
      for (let i = 0; i < index.length; i++) {
        const e = index[i]!;
        const s = fuzzyScore(fileSearchQuery, e);
        if (s >= 0) {
          const slash = e.path.lastIndexOf('/');
          scored.push({ path: e.path, basename: e.basename, dir: slash >= 0 ? e.path.slice(0, slash) : '', score: s });
        }
        if (scored.length >= 500) break;
      }
      scored.sort((a, b) => b.score - a.score);
      setFileSearchResults(scored.slice(0, 50));
      setSelectedIdx(0);
    }, 15);
    return () => clearTimeout(searchTimerRef.current);
  }, [fileSearchQuery, showFileSearch]);

  // Grep search
  useEffect(() => {
    if (!showGrepPanel || grepQuery.length < 2) { setGrepResults([]); return; }
    clearTimeout(grepTimerRef.current);
    grepTimerRef.current = setTimeout(async () => {
      setGrepSearching(true);
      try {
        const results = await api.fileGrep(p, n, grepQuery, grepRegex);
        setGrepResults(results);
      } catch { setGrepResults([]); }
      finally { setGrepSearching(false); }
    }, 400);
    return () => clearTimeout(grepTimerRef.current);
  }, [grepQuery, grepRegex, showGrepPanel, p, n]);

  const handleBeforeMount: BeforeMount = useCallback((m: Monaco) => {
    installMonacoCancelHandler();
    setupJsxSupport(m, ALL_EDITOR_THEMES);

    const ts = m.languages.typescript;
    const diagOpts = { noSemanticValidation: true, noSyntaxValidation: true, noSuggestionDiagnostics: true };
    ts.typescriptDefaults.setDiagnosticsOptions(diagOpts);
    ts.javascriptDefaults.setDiagnosticsOptions(diagOpts);

    const compilerOpts = {
      target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.NodeJs, jsx: ts.JsxEmit.ReactJSX,
      allowJs: true, allowNonTsExtensions: true, esModuleInterop: true, skipLibCheck: true, strict: false,
    };
    ts.typescriptDefaults.setCompilerOptions(compilerOpts);
    ts.javascriptDefaults.setCompilerOptions(compilerOpts);

    // Disable built-in JSON validation too -- vscode-json-language-server handles it via LSP.
    // Still allow comments/trailing commas for tokenization (tsconfig.json etc.)
    m.languages.json.jsonDefaults.setDiagnosticsOptions({
      validate: false, allowComments: true, trailingCommas: 'ignore', schemaValidation: 'ignore',
    });
  }, []);

  const handleEditorMount: OnMount = useCallback((editor, monaco) => {
    editorRef.current = editor;
    monacoRef.current = monaco;
    setMonacoReady(true);
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyP, () => {});
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyF, () => {});
    // On mount/remount, switch to the active file's model
    const ap = activePathRef.current;
    if (ap != null) {
      const cached = tabContentsRef.current.get(ap);
      switchEditorModel(editor, monaco, `file://${ap}`, cached?.content ?? '', extToLanguage(ap));
    }
    if (lspBridgeRef.current != null) {
      lspBridgeRef.current.setEditor(editor);
      const model = editor.getModel();
      if (model != null) lspBridgeRef.current.setModel(model, ap != null ? `file://${ap}` : undefined);
    }
  }, []);

  // Keep editor model and LSP bridge in sync when activePath or LSP connection changes
  useEffect(() => {
    const ed = editorRef.current;
    const m = monacoRef.current;
    if (ed == null || m == null || activePath == null) return;
    // Skip if the editor has been disposed (stale ref after unmount/remount)
    try { ed.getDomNode(); } catch { return; }
    if (ed.getDomNode() == null) return;
    // Switch model if the editor isn't already showing the right file
    const targetUri = m.Uri.parse(`file://${activePath}`).toString();
    if (ed.getModel()?.uri?.toString() !== targetUri) {
      const cached = tabContentsRef.current.get(activePath);
      switchEditorModel(ed, m, `file://${activePath}`, cached?.content ?? content, extToLanguage(activePath));
    }
    if (lspBridgeRef.current != null) {
      lspBridgeRef.current.setEditor(ed);
      const model = ed.getModel();
      if (model != null) lspBridgeRef.current.setModel(model, `file://${activePath}`);
    }
  }, [activePath, lspConnected, monacoReady, content]);

  // Keep tabContents in sync with editor edits
  useEffect(() => {
    if (activePath != null) {
      tabContentsRef.current.set(activePath, { content, saved: savedContent });
    }
  }, [content, savedContent, activePath]);

  const language = activePath != null ? extToLanguage(activePath) : 'plaintext';

  // Capture the first non-null activePath so the <Editor> path prop never changes.
  if (activePath != null && initialEditorPathRef.current == null) {
    initialEditorPathRef.current = `file://${activePath}`;
  }

  // Derive chrome colors from the active editor theme so the surrounding UI matches
  const isLightTheme = activeTheme.base === 'vs';
  const editorBg = activeTheme.colors['editor.background'] ?? (isLightTheme ? '#ffffff' : '#1e1e1e');
  const editorFg = activeTheme.colors['editor.foreground'] ?? (isLightTheme ? '#24292f' : '#cccccc');
  const lineNumFg = activeTheme.colors['editorLineNumber.foreground'] ?? (isLightTheme ? '#8b949e' : '#6e7681');
  const lineNumActiveFg = activeTheme.colors['editorLineNumber.activeForeground'] ?? (isLightTheme ? '#57606a' : '#8b949e');

  function adjustHex(hex: string, amount: number): string {
    const h = hex.replace('#', '');
    const r = Math.min(255, Math.max(0, parseInt(h.substring(0, 2), 16) + amount));
    const g = Math.min(255, Math.max(0, parseInt(h.substring(2, 4), 16) + amount));
    const b = Math.min(255, Math.max(0, parseInt(h.substring(4, 6), 16) + amount));
    return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`;
  }

  const shift = isLightTheme ? -1 : 1;
  const chromeStyle = {
    toolbarBg: adjustHex(editorBg, shift * 8),
    tabBarBg: adjustHex(editorBg, shift * -4),
    tabActiveBg: editorBg,
    tabHoverBg: adjustHex(editorBg, shift * 12),
    sidebarBg: adjustHex(editorBg, shift * -2),
    borderColor: adjustHex(editorBg, shift * 20),
    textColor: editorFg,
    textMuted: lineNumActiveFg,
    textSubtle: lineNumFg,
    hoverBg: adjustHex(editorBg, shift * 16),
  };

  // Disambiguate tab names when two files share the same basename
  const tabLabels = openTabs.map((tabPath) => {
    const bn = basename(tabPath);
    const dupes = openTabs.filter((t) => basename(t) === bn);
    if (dupes.length > 1) {
      const dir = dirname(tabPath).replace('/workspace/', '').split('/').pop() ?? '';
      return dir.length > 0 ? `${bn} — ${dir}` : bn;
    }
    return bn;
  });

  return (
    <div
      className={
        fullscreen
          ? 'fixed inset-0 z-[200] flex flex-col'
          : 'flex flex-col rounded-xl overflow-hidden border border-[var(--border)]'
      }
      style={{ background: editorBg, ...(fullscreen ? {} : { height: 'calc(100vh - 340px)', minHeight: '400px' }) }}
    >
      {/* Toolbar */}
      <div className="flex items-center gap-2 px-3 py-1.5 border-b shrink-0"
        style={{ background: chromeStyle.toolbarBg, borderColor: chromeStyle.borderColor, color: chromeStyle.textColor }}>
        <button type="button" onClick={() => setSidebarOpen((v) => !v)}
          className="h-7 w-7 inline-flex items-center justify-center rounded text-subtle-ui hover:text-main hover:bg-white/10 transition-colors"
          title={t('files.toggleSidebar')}>
          <SidebarSimple size={16} />
        </button>
        <div className="h-4 w-px" style={{ background: chromeStyle.borderColor }} />
        {activePath != null ? (
          <span className="text-xs font-mono truncate flex-1" style={{ color: chromeStyle.textMuted }} title={activePath}>
            {activePath.replace('/workspace/', '')}
          </span>
        ) : (
          <span className="text-xs flex-1" style={{ color: chromeStyle.textSubtle }}>{t('files.noFileOpen')}</span>
        )}
        {isDirty && (
          <span className="inline-flex items-center gap-1 text-[10px] text-amber-500">
            <Circle size={8} weight="fill" /> {t('files.unsaved')}
          </span>
        )}
        <button type="button" onClick={() => void handleSave()} disabled={!isDirty || saving}
          className="btn btn-outline !px-2 !py-1 !text-xs inline-flex items-center gap-1.5 disabled:opacity-30" title="Ctrl+S">
          <FloppyDisk size={14} /> {saving ? t('files.saving') : t('files.save')}
        </button>
        <button type="button" onClick={() => { setShowFileSearch(true); setShowGrepPanel(false); }}
          className="h-7 w-7 inline-flex items-center justify-center rounded text-subtle-ui hover:text-main hover:bg-white/10 transition-colors" title="Ctrl+P">
          <MagnifyingGlass size={16} />
        </button>
        <button type="button" onClick={() => { setShowGrepPanel((v) => !v); setShowFileSearch(false); }}
          className={`h-7 w-7 inline-flex items-center justify-center rounded transition-colors ${showGrepPanel ? 'text-[var(--primary)] bg-[var(--primary)]/10' : 'text-subtle-ui hover:text-main hover:bg-white/10'}`}
          title="Ctrl+Shift+F">
          <TextT size={16} />
        </button>
        <EditorThemePicker themes={editorThemes} activeId={activeTheme.id} onSelect={setEditorTheme} />
        <button type="button" onClick={() => setFullscreen((v) => !v)}
          className="h-7 w-7 inline-flex items-center justify-center rounded text-subtle-ui hover:text-main hover:bg-white/10 transition-colors"
          title={fullscreen ? t('files.exitFullscreen') : t('files.fullscreen')}>
          {fullscreen ? <CornersIn size={16} /> : <CornersOut size={16} />}
        </button>
      </div>

      {/* Tab bar */}
      {openTabs.length > 0 && (
        <div className="flex items-center border-b shrink-0 overflow-x-auto"
          style={{ scrollbarWidth: 'none', background: chromeStyle.tabBarBg, borderColor: chromeStyle.borderColor }}>
          {openTabs.map((tabPath, i) => {
            const isActive = tabPath === activePath;
            const cached = tabContentsRef.current.get(tabPath);
            const tabDirty = cached != null && cached.content !== cached.saved;
            return (
              <button key={tabPath} type="button"
                ref={(el) => { if (isActive && el != null) el.scrollIntoView({ block: 'nearest', inline: 'nearest' }); }}
                className="group flex items-center gap-1.5 px-3 py-1.5 text-xs font-mono shrink-0 transition-colors"
                style={{
                  borderRight: `1px solid ${chromeStyle.borderColor}`,
                  background: isActive ? chromeStyle.tabActiveBg : 'transparent',
                  color: isActive ? chromeStyle.textColor : chromeStyle.textMuted,
                  borderBottom: isActive ? '2px solid var(--primary)' : '2px solid transparent',
                }}
                onClick={() => switchToTab(tabPath)}
                onMouseDown={(e) => { if (e.button === 1) { e.preventDefault(); closeTab(tabPath); } }}>
                <span className="truncate max-w-[160px]">{tabLabels[i]}</span>
                {tabDirty ? (
                  <Circle size={8} weight="fill" className="text-amber-500 shrink-0" />
                ) : (
                  <span className="w-4 h-4 inline-flex items-center justify-center rounded-sm opacity-0 group-hover:opacity-100 shrink-0"
                    style={{ color: chromeStyle.textMuted }}
                    onClick={(e) => { e.stopPropagation(); closeTab(tabPath); }}>
                    <X size={10} />
                  </span>
                )}
              </button>
            );
          })}
        </div>
      )}

      {/* Main content area */}
      <div className="flex flex-1 min-h-0">
        {/* File tree / grep results sidebar — kept mounted to preserve tree state */}
        <div
          className="border-r overflow-hidden shrink-0 flex flex-col"
          style={{
            width: (sidebarOpen || showGrepPanel) ? '260px' : '0px',
            display: (sidebarOpen || showGrepPanel) ? undefined : 'none',
            background: chromeStyle.sidebarBg,
            borderColor: chromeStyle.borderColor,
            color: chromeStyle.textColor,
          }}
        >
            {showGrepPanel ? (
              <>
                <div className="p-2 border-b" style={{ borderColor: chromeStyle.borderColor }}>
                  <div className="flex items-center gap-1.5 h-7 px-2 rounded-md border bg-transparent" style={{ borderColor: chromeStyle.borderColor }}>
                    <MagnifyingGlass size={14} className="text-subtle-ui shrink-0" />
                    <input ref={grepInputRef} type="text" value={grepQuery} onChange={(e) => setGrepQuery(e.target.value)}
                      placeholder={t('files.grepPlaceholder')}
                      className="flex-1 bg-transparent text-xs outline-none min-w-0"
                      style={{ color: chromeStyle.textColor }} />
                    <button type="button" onClick={() => setGrepRegex((v) => !v)}
                      className={`h-5 px-1 text-[10px] font-semibold rounded border transition-colors ${grepRegex ? 'border-[var(--primary)] text-[var(--primary)] bg-[var(--primary)]/10' : 'border-transparent text-subtle-ui hover:text-main'}`}
                      title="Regex">
                      <Asterisk size={12} />
                    </button>
                  </div>
                  <div className="flex items-center justify-between mt-1.5 px-1">
                    <span className="text-[10px] text-subtle-ui">
                      {grepSearching ? 'Searching...' : `${grepResults.length} results`}
                    </span>
                    <button type="button" onClick={() => setShowGrepPanel(false)} className="text-subtle-ui hover:text-main">
                      <X size={12} />
                    </button>
                  </div>
                </div>
                <div className="flex-1 overflow-y-auto">
                  {grepResults.length === 0 && grepQuery.length >= 2 && !grepSearching && (
                    <div className="px-3 py-4 text-xs text-subtle-ui text-center">{t('files.noResults')}</div>
                  )}
                  {grepResults.map((r, i) => (
                    <button key={`${r.path}:${r.line}:${i}`} type="button"
                      className="w-full text-left px-2 py-1 block last:border-0"
                      style={{ borderBottom: `1px solid ${chromeStyle.borderColor}` }}
                      onClick={() => void openFileInTab(r.path, r.line)}>
                      <div className="text-[10px] font-mono text-[var(--primary)] truncate pr-4" dir="rtl"><bdi>{r.path}</bdi></div>
                      <div className="flex items-center gap-1.5">
                        <span className="text-[10px] text-subtle-ui shrink-0">{r.line}</span>
                        <span className="text-[10px] font-mono text-main truncate">{r.text}</span>
                      </div>
                    </button>
                  ))}
                </div>
              </>
            ) : (
              <>
                <div className="text-[10px] font-semibold uppercase tracking-wider px-3 py-2 border-b"
                  style={{ color: chromeStyle.textSubtle, borderColor: chromeStyle.borderColor }}>
                  {t('files.explorer')}
                </div>
                <FileTree
                  ref={fileTreeRef}
                  project={p}
                  name={n}
                  rootPath="/workspace"
                  activePath={activePath}
                  onFileSelect={(path) => void openFileInTab(path)}
                  gitStatus={gitStatus}
                  isLight={isLightTheme}
                  initialExpanded={restored ? expandedPaths : undefined}
                  onExpandedChange={setExpandedPaths}
                />
              </>
            )}
          </div>

        {/* Editor */}
        <div className="flex-1 min-w-0">
          {loading ? (
            <div className="flex items-center justify-center h-full text-sm" style={{ color: chromeStyle.textSubtle, background: editorBg }}>{t('files.loading')}</div>
          ) : error != null ? (
            <div className="flex items-center justify-center h-full text-rose-500 text-sm px-4 text-center" style={{ background: editorBg }}>{error}</div>
          ) : activePath == null ? (
            <div className="flex items-center justify-center h-full text-sm" style={{ color: chromeStyle.textSubtle, background: editorBg }}>
              <div className="text-center">
                <p>{t('files.selectFile')}</p>
                <p className="text-xs mt-1" style={{ opacity: 0.6 }}>{t('files.ctrlPHint')}</p>
              </div>
            </div>
          ) : (
            <Editor
              path={initialEditorPathRef.current!}
              keepCurrentModel
              language={language} value={content} onChange={(v) => setContent(v ?? '')}
              beforeMount={handleBeforeMount} onMount={handleEditorMount} theme={activeTheme.id}
              options={{
                'semanticHighlighting.enabled': true,
                fontSize: 13,
                fontFamily: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', Menlo, monospace",
                minimap: { enabled: fullscreen }, lineNumbers: 'on', scrollBeyondLastLine: false,
                wordWrap: 'on', padding: { top: 8 }, renderLineHighlight: 'line',
                cursorBlinking: 'smooth', smoothScrolling: true, tabSize: 2,
              }}
            />
          )}
        </div>
      </div>

      {/* Ctrl+P Fuzzy File Search Modal — rendered via Portal for full-page coverage */}
      {showFileSearch && createPortal(
        <div className="fixed inset-0 z-[300] flex items-start justify-center pt-[15vh] bg-black/50 backdrop-blur-sm"
          onMouseDown={() => setShowFileSearch(false)}>
          <div className="w-[560px] max-w-[90vw] glass-panel shadow-2xl overflow-hidden"
            onMouseDown={(e) => e.stopPropagation()}>
            <div className="flex items-center gap-2 px-3 py-2 border-b border-[var(--border)]">
              <MagnifyingGlass size={16} className="text-subtle-ui shrink-0" />
              <input ref={fileSearchRef} type="text" value={fileSearchQuery}
                onChange={(e) => setFileSearchQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Escape') { setShowFileSearch(false); return; }
                  if (e.key === 'ArrowDown') { e.preventDefault(); setSelectedIdx((i) => Math.min(i + 1, fileSearchResults.length - 1)); return; }
                  if (e.key === 'ArrowUp') { e.preventDefault(); setSelectedIdx((i) => Math.max(i - 1, 0)); return; }
                  if (e.key === 'Enter' && fileSearchResults.length > 0) {
                    void openFileInTab(fileSearchResults[selectedIdx]?.path ?? fileSearchResults[0]!.path);
                    setShowFileSearch(false);
                  }
                }}
                placeholder={t('files.searchPlaceholder')}
                className="flex-1 bg-transparent text-sm text-main outline-none placeholder:text-subtle-ui"
                autoComplete="off" spellCheck={false} />
              {indexLoading ? (
                <span className="text-[10px] text-subtle-ui animate-pulse shrink-0">indexing...</span>
              ) : (
                <span className="text-[10px] text-subtle-ui shrink-0">{indexCount.toLocaleString()} files</span>
              )}
              <button type="button" onClick={() => setShowFileSearch(false)} className="text-subtle-ui hover:text-main shrink-0">
                <X size={16} />
              </button>
            </div>
            <div className="max-h-[400px] overflow-y-auto">
              {fileSearchResults.length === 0 && fileSearchQuery.length >= 1 && !indexLoading && (
                <div className="px-3 py-4 text-sm text-subtle-ui text-center">{t('files.noResults')}</div>
              )}
              {fileSearchResults.map((r, i) => (
                <button key={r.path} type="button"
                  className={`w-full text-left px-3 py-1.5 flex items-center gap-2 transition-colors ${i === selectedIdx ? 'bg-[var(--primary)]/15 text-main' : 'hover:bg-white/5'}`}
                  onClick={() => { void openFileInTab(r.path); setShowFileSearch(false); }}
                  onMouseEnter={() => setSelectedIdx(i)}>
                  <span className="text-xs font-mono text-main truncate">{r.basename}</span>
                  {r.dir.length > 0 && <span className="text-[10px] font-mono text-subtle-ui truncate">{r.dir}</span>}
                </button>
              ))}
            </div>
          </div>
        </div>,
        document.body,
      )}
    </div>
  );
}
