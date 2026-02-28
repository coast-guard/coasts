import { useCallback, useEffect, useState } from 'react';
import { api } from '../api/endpoints';
import { useTheme } from '../providers/ThemeProvider';

export interface EditorThemeTokenRule {
  readonly token: string;
  readonly foreground?: string;
  readonly fontStyle?: string;
}

export interface EditorThemeDef {
  readonly id: string;
  readonly labelKey: string;
  readonly mode: 'light' | 'dark';
  readonly base: 'vs' | 'vs-dark';
  readonly rules: readonly EditorThemeTokenRule[];
  readonly colors: Record<string, string>;
}

// ---------------------------------------------------------------------------
// JSX token rules shared across all themes (with per-theme color overrides)
// ---------------------------------------------------------------------------

function jsxRules(tag: string, delim: string, attr: string, attrVal: string, htmlStr: string): EditorThemeTokenRule[] {
  return [
    { token: 'tag', foreground: tag },
    { token: 'delimiter.html', foreground: delim },
    { token: 'attribute.name', foreground: attr },
    { token: 'attribute.value', foreground: attrVal },
    { token: 'string.html', foreground: htmlStr },
  ];
}

// ---------------------------------------------------------------------------
// Dark themes
// ---------------------------------------------------------------------------

const DARK_THEMES: readonly EditorThemeDef[] = [
  {
    id: 'coast-dark',
    labelKey: 'editorTheme.vscodeDark',
    mode: 'dark',
    base: 'vs-dark',
    rules: jsxRules('569cd6', '808080', '9cdcfe', 'ce9178', 'd4d4d4'),
    colors: {},
  },
  {
    id: 'coast-dracula',
    labelKey: 'editorTheme.dracula',
    mode: 'dark',
    base: 'vs-dark',
    rules: [
      { token: 'keyword', foreground: 'ff79c6' },
      { token: 'identifier', foreground: 'f8f8f2' },
      { token: 'type.identifier', foreground: '8be9fd', fontStyle: 'italic' },
      { token: 'string', foreground: 'f1fa8c' },
      { token: 'number', foreground: 'bd93f9' },
      { token: 'number.float', foreground: 'bd93f9' },
      { token: 'number.hex', foreground: 'bd93f9' },
      { token: 'comment', foreground: '6272a4' },
      { token: 'comment.doc', foreground: '6272a4' },
      { token: 'regexp', foreground: 'ff5555' },
      { token: 'delimiter', foreground: 'f8f8f2' },
      ...jsxRules('8be9fd', '6272a4', '50fa7b', 'f1fa8c', 'f8f8f2'),
    ],
    colors: {
      'editor.background': '#282a36',
      'editor.foreground': '#f8f8f2',
      'editor.lineHighlightBackground': '#44475a55',
      'editor.selectionBackground': '#44475a',
      'editorCursor.foreground': '#f8f8f2',
      'editorLineNumber.foreground': '#6272a4',
      'editorLineNumber.activeForeground': '#f8f8f2',
    },
  },
  {
    id: 'coast-tokyoNight',
    labelKey: 'editorTheme.tokyoNight',
    mode: 'dark',
    base: 'vs-dark',
    rules: [
      { token: 'keyword', foreground: 'bb9af7' },
      { token: 'identifier', foreground: 'c0caf5' },
      { token: 'type.identifier', foreground: '2ac3de' },
      { token: 'string', foreground: '9ece6a' },
      { token: 'number', foreground: 'ff9e64' },
      { token: 'number.float', foreground: 'ff9e64' },
      { token: 'number.hex', foreground: 'ff9e64' },
      { token: 'comment', foreground: '565f89' },
      { token: 'comment.doc', foreground: '565f89' },
      { token: 'regexp', foreground: 'f7768e' },
      { token: 'delimiter', foreground: '89ddff' },
      ...jsxRules('7aa2f7', '565f89', '73daca', '9ece6a', 'c0caf5'),
    ],
    colors: {
      'editor.background': '#1a1b26',
      'editor.foreground': '#c0caf5',
      'editor.lineHighlightBackground': '#292e4255',
      'editor.selectionBackground': '#33467c',
      'editorCursor.foreground': '#c0caf5',
      'editorLineNumber.foreground': '#3b4261',
      'editorLineNumber.activeForeground': '#737aa2',
    },
  },
  {
    id: 'coast-nord',
    labelKey: 'editorTheme.nord',
    mode: 'dark',
    base: 'vs-dark',
    rules: [
      { token: 'keyword', foreground: '81a1c1' },
      { token: 'identifier', foreground: 'd8dee9' },
      { token: 'type.identifier', foreground: '8fbcbb' },
      { token: 'string', foreground: 'a3be8c' },
      { token: 'number', foreground: 'b48ead' },
      { token: 'number.float', foreground: 'b48ead' },
      { token: 'number.hex', foreground: 'b48ead' },
      { token: 'comment', foreground: '616e88' },
      { token: 'comment.doc', foreground: '616e88' },
      { token: 'regexp', foreground: 'ebcb8b' },
      { token: 'delimiter', foreground: 'eceff4' },
      ...jsxRules('81a1c1', '4c566a', '8fbcbb', 'a3be8c', 'd8dee9'),
    ],
    colors: {
      'editor.background': '#2e3440',
      'editor.foreground': '#d8dee9',
      'editor.lineHighlightBackground': '#3b425255',
      'editor.selectionBackground': '#434c5e',
      'editorCursor.foreground': '#d8dee9',
      'editorLineNumber.foreground': '#4c566a',
      'editorLineNumber.activeForeground': '#d8dee9',
    },
  },
];

// ---------------------------------------------------------------------------
// Light themes
// ---------------------------------------------------------------------------

const LIGHT_THEMES: readonly EditorThemeDef[] = [
  {
    id: 'coast-light',
    labelKey: 'editorTheme.vscodeLight',
    mode: 'light',
    base: 'vs',
    rules: jsxRules('0000ff', '800000', 'e50000', '0000ff', '000000'),
    colors: {},
  },
  {
    id: 'coast-githubLight',
    labelKey: 'editorTheme.githubLight',
    mode: 'light',
    base: 'vs',
    rules: [
      { token: 'keyword', foreground: 'cf222e' },
      { token: 'identifier', foreground: '24292f' },
      { token: 'type.identifier', foreground: '953800' },
      { token: 'string', foreground: '0a3069' },
      { token: 'number', foreground: '0550ae' },
      { token: 'number.float', foreground: '0550ae' },
      { token: 'number.hex', foreground: '0550ae' },
      { token: 'comment', foreground: '6e7781' },
      { token: 'comment.doc', foreground: '6e7781' },
      { token: 'regexp', foreground: '0550ae' },
      { token: 'delimiter', foreground: '24292f' },
      ...jsxRules('0550ae', '6e7781', '116329', '0a3069', '24292f'),
    ],
    colors: {
      'editor.background': '#ffffff',
      'editor.foreground': '#24292f',
      'editor.lineHighlightBackground': '#f6f8fa',
      'editor.selectionBackground': '#ddf4ff',
      'editorCursor.foreground': '#044289',
      'editorLineNumber.foreground': '#8c959f',
      'editorLineNumber.activeForeground': '#24292f',
    },
  },
  {
    id: 'coast-solarizedLight',
    labelKey: 'editorTheme.solarizedLight',
    mode: 'light',
    base: 'vs',
    rules: [
      { token: 'keyword', foreground: '859900' },
      { token: 'identifier', foreground: '657b83' },
      { token: 'type.identifier', foreground: 'b58900' },
      { token: 'string', foreground: '2aa198' },
      { token: 'number', foreground: 'd33682' },
      { token: 'number.float', foreground: 'd33682' },
      { token: 'number.hex', foreground: 'd33682' },
      { token: 'comment', foreground: '93a1a1' },
      { token: 'comment.doc', foreground: '93a1a1' },
      { token: 'regexp', foreground: 'dc322f' },
      { token: 'delimiter', foreground: '586e75' },
      ...jsxRules('268bd2', '93a1a1', '2aa198', '2aa198', '657b83'),
    ],
    colors: {
      'editor.background': '#fdf6e3',
      'editor.foreground': '#657b83',
      'editor.lineHighlightBackground': '#eee8d5',
      'editor.selectionBackground': '#eee8d5',
      'editorCursor.foreground': '#586e75',
      'editorLineNumber.foreground': '#93a1a1',
      'editorLineNumber.activeForeground': '#586e75',
    },
  },
  {
    id: 'coast-quietLight',
    labelKey: 'editorTheme.quietLight',
    mode: 'light',
    base: 'vs',
    rules: [
      { token: 'keyword', foreground: '4b69c6' },
      { token: 'identifier', foreground: '333333' },
      { token: 'type.identifier', foreground: '7a3e9d' },
      { token: 'string', foreground: '448c27' },
      { token: 'number', foreground: 'ab6526' },
      { token: 'number.float', foreground: 'ab6526' },
      { token: 'number.hex', foreground: 'ab6526' },
      { token: 'comment', foreground: 'aaaaaa' },
      { token: 'comment.doc', foreground: 'aaaaaa' },
      { token: 'regexp', foreground: 'ab6526' },
      { token: 'delimiter', foreground: '777777' },
      ...jsxRules('4b69c6', 'aaaaaa', '7a3e9d', '448c27', '333333'),
    ],
    colors: {
      'editor.background': '#f5f5f5',
      'editor.foreground': '#333333',
      'editor.lineHighlightBackground': '#e4f6d4',
      'editor.selectionBackground': '#c9d0d9',
      'editorCursor.foreground': '#54494b',
      'editorLineNumber.foreground': '#aaaaaa',
      'editorLineNumber.activeForeground': '#333333',
    },
  },
];

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------

export const ALL_EDITOR_THEMES: readonly EditorThemeDef[] = [...DARK_THEMES, ...LIGHT_THEMES];

export function getEditorThemesByMode(mode: 'light' | 'dark'): readonly EditorThemeDef[] {
  return mode === 'dark' ? DARK_THEMES : LIGHT_THEMES;
}

export function getEditorThemeById(id: string): EditorThemeDef | undefined {
  return ALL_EDITOR_THEMES.find((t) => t.id === id);
}

const DEFAULT_DARK = 'coast-dark';
const DEFAULT_LIGHT = 'coast-light';

export function useEditorTheme() {
  const { theme: appTheme } = useTheme();

  const [darkThemeId, setDarkThemeId] = useState<string>(() =>
    localStorage.getItem('coast-editor-theme-dark') ?? DEFAULT_DARK,
  );
  const [lightThemeId, setLightThemeId] = useState<string>(() =>
    localStorage.getItem('coast-editor-theme-light') ?? DEFAULT_LIGHT,
  );

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [dark, light] = await Promise.all([
        api.getSetting('editor_theme_dark'),
        api.getSetting('editor_theme_light'),
      ]);
      if (cancelled) return;
      if (dark != null && getEditorThemeById(dark) != null) {
        setDarkThemeId(dark);
        localStorage.setItem('coast-editor-theme-dark', dark);
      }
      if (light != null && getEditorThemeById(light) != null) {
        setLightThemeId(light);
        localStorage.setItem('coast-editor-theme-light', light);
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const activeId = appTheme === 'dark' ? darkThemeId : lightThemeId;
  const activeTheme = getEditorThemeById(activeId) ?? getEditorThemeById(appTheme === 'dark' ? DEFAULT_DARK : DEFAULT_LIGHT)!;

  const setEditorTheme = useCallback(
    (id: string) => {
      const def = getEditorThemeById(id);
      if (def == null) return;
      if (def.mode === 'dark') {
        setDarkThemeId(id);
        localStorage.setItem('coast-editor-theme-dark', id);
        void api.setSetting('editor_theme_dark', id);
      } else {
        setLightThemeId(id);
        localStorage.setItem('coast-editor-theme-light', id);
        void api.setSetting('editor_theme_light', id);
      }
    },
    [],
  );

  return {
    activeTheme,
    appTheme,
    setEditorTheme,
    themes: getEditorThemesByMode(appTheme),
  };
}
