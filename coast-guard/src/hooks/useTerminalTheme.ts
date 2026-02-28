import { useCallback, useEffect, useState } from 'react';
import { api } from '../api/endpoints';
import { useTheme } from '../providers/ThemeProvider';

export interface TerminalColorScheme {
  readonly background: string;
  readonly foreground: string;
  readonly cursor: string;
  readonly selectionBackground: string;
  readonly black: string;
  readonly red: string;
  readonly green: string;
  readonly yellow: string;
  readonly blue: string;
  readonly magenta: string;
  readonly cyan: string;
  readonly white: string;
  readonly brightBlack: string;
  readonly brightRed: string;
  readonly brightGreen: string;
  readonly brightYellow: string;
  readonly brightBlue: string;
  readonly brightMagenta: string;
  readonly brightCyan: string;
  readonly brightWhite: string;
}

export interface TerminalThemeDef {
  readonly id: string;
  readonly labelKey: string;
  readonly mode: 'light' | 'dark';
  readonly colors: TerminalColorScheme;
}

const DARK_THEMES: readonly TerminalThemeDef[] = [
  {
    id: 'midnight',
    labelKey: 'termTheme.midnight',
    mode: 'dark',
    colors: {
      background: '#0b1222', foreground: '#e2e8f0', cursor: '#60a5fa', selectionBackground: '#1e293b',
      black: '#1e293b', red: '#f87171', green: '#4ade80', yellow: '#facc15', blue: '#60a5fa', magenta: '#c084fc', cyan: '#22d3ee', white: '#e2e8f0',
      brightBlack: '#475569', brightRed: '#fca5a5', brightGreen: '#86efac', brightYellow: '#fde68a', brightBlue: '#93c5fd', brightMagenta: '#d8b4fe', brightCyan: '#67e8f9', brightWhite: '#f8fafc',
    },
  },
  {
    id: 'dracula',
    labelKey: 'termTheme.dracula',
    mode: 'dark',
    colors: {
      background: '#282a36', foreground: '#f8f8f2', cursor: '#f8f8f2', selectionBackground: '#44475a',
      black: '#21222c', red: '#ff5555', green: '#50fa7b', yellow: '#f1fa8c', blue: '#bd93f9', magenta: '#ff79c6', cyan: '#8be9fd', white: '#f8f8f2',
      brightBlack: '#6272a4', brightRed: '#ff6e6e', brightGreen: '#69ff94', brightYellow: '#ffffa5', brightBlue: '#d6acff', brightMagenta: '#ff92df', brightCyan: '#a4ffff', brightWhite: '#ffffff',
    },
  },
  {
    id: 'tokyoNight',
    labelKey: 'termTheme.tokyoNight',
    mode: 'dark',
    colors: {
      background: '#1a1b26', foreground: '#c0caf5', cursor: '#c0caf5', selectionBackground: '#33467c',
      black: '#15161e', red: '#f7768e', green: '#9ece6a', yellow: '#e0af68', blue: '#7aa2f7', magenta: '#bb9af7', cyan: '#7dcfff', white: '#a9b1d6',
      brightBlack: '#414868', brightRed: '#f7768e', brightGreen: '#9ece6a', brightYellow: '#e0af68', brightBlue: '#7aa2f7', brightMagenta: '#bb9af7', brightCyan: '#7dcfff', brightWhite: '#c0caf5',
    },
  },
  {
    id: 'nord',
    labelKey: 'termTheme.nord',
    mode: 'dark',
    colors: {
      background: '#2e3440', foreground: '#d8dee9', cursor: '#d8dee9', selectionBackground: '#434c5e',
      black: '#3b4252', red: '#bf616a', green: '#a3be8c', yellow: '#ebcb8b', blue: '#81a1c1', magenta: '#b48ead', cyan: '#88c0d0', white: '#e5e9f0',
      brightBlack: '#4c566a', brightRed: '#bf616a', brightGreen: '#a3be8c', brightYellow: '#ebcb8b', brightBlue: '#81a1c1', brightMagenta: '#b48ead', brightCyan: '#8fbcbb', brightWhite: '#eceff4',
    },
  },
];

const LIGHT_THEMES: readonly TerminalThemeDef[] = [
  {
    id: 'cloud',
    labelKey: 'termTheme.cloud',
    mode: 'light',
    colors: {
      background: '#f0f4fa', foreground: '#0f172a', cursor: '#2563eb', selectionBackground: '#bfdbfe',
      black: '#1e293b', red: '#dc2626', green: '#16a34a', yellow: '#ca8a04', blue: '#2563eb', magenta: '#9333ea', cyan: '#0891b2', white: '#f1f5f9',
      brightBlack: '#64748b', brightRed: '#ef4444', brightGreen: '#22c55e', brightYellow: '#eab308', brightBlue: '#3b82f6', brightMagenta: '#a855f7', brightCyan: '#06b6d4', brightWhite: '#ffffff',
    },
  },
  {
    id: 'paper',
    labelKey: 'termTheme.paper',
    mode: 'light',
    colors: {
      background: '#fafaf9', foreground: '#1c1917', cursor: '#1c1917', selectionBackground: '#e7e5e4',
      black: '#1c1917', red: '#dc2626', green: '#16a34a', yellow: '#a16207', blue: '#2563eb', magenta: '#7c3aed', cyan: '#0e7490', white: '#f5f5f4',
      brightBlack: '#78716c', brightRed: '#ef4444', brightGreen: '#22c55e', brightYellow: '#ca8a04', brightBlue: '#3b82f6', brightMagenta: '#8b5cf6', brightCyan: '#0891b2', brightWhite: '#ffffff',
    },
  },
  {
    id: 'solarizedLight',
    labelKey: 'termTheme.solarizedLight',
    mode: 'light',
    colors: {
      background: '#fdf6e3', foreground: '#657b83', cursor: '#586e75', selectionBackground: '#eee8d5',
      black: '#073642', red: '#dc322f', green: '#859900', yellow: '#b58900', blue: '#268bd2', magenta: '#d33682', cyan: '#2aa198', white: '#eee8d5',
      brightBlack: '#002b36', brightRed: '#cb4b16', brightGreen: '#586e75', brightYellow: '#657b83', brightBlue: '#839496', brightMagenta: '#6c71c4', brightCyan: '#93a1a1', brightWhite: '#fdf6e3',
    },
  },
  {
    id: 'github',
    labelKey: 'termTheme.github',
    mode: 'light',
    colors: {
      background: '#ffffff', foreground: '#24292f', cursor: '#044289', selectionBackground: '#ddf4ff',
      black: '#24292f', red: '#cf222e', green: '#116329', yellow: '#4d2d00', blue: '#0969da', magenta: '#8250df', cyan: '#1b7c83', white: '#f6f8fa',
      brightBlack: '#57606a', brightRed: '#a40e26', brightGreen: '#1a7f37', brightYellow: '#633c01', brightBlue: '#218bff', brightMagenta: '#a475f9', brightCyan: '#3192aa', brightWhite: '#ffffff',
    },
  },
];

export const ALL_TERMINAL_THEMES: readonly TerminalThemeDef[] = [...DARK_THEMES, ...LIGHT_THEMES];

export function getTerminalThemesByMode(mode: 'light' | 'dark'): readonly TerminalThemeDef[] {
  return mode === 'dark' ? DARK_THEMES : LIGHT_THEMES;
}

export function getTerminalThemeById(id: string): TerminalThemeDef | undefined {
  return ALL_TERMINAL_THEMES.find((t) => t.id === id);
}

const DEFAULT_DARK = 'midnight';
const DEFAULT_LIGHT = 'cloud';

export function useTerminalTheme() {
  const { theme: appTheme } = useTheme();

  const [darkThemeId, setDarkThemeId] = useState<string>(() =>
    localStorage.getItem('coast-term-theme-dark') ?? DEFAULT_DARK,
  );
  const [lightThemeId, setLightThemeId] = useState<string>(() =>
    localStorage.getItem('coast-term-theme-light') ?? DEFAULT_LIGHT,
  );

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [dark, light] = await Promise.all([
        api.getSetting('terminal_theme_dark'),
        api.getSetting('terminal_theme_light'),
      ]);
      if (cancelled) return;
      if (dark != null && getTerminalThemeById(dark) != null) {
        setDarkThemeId(dark);
        localStorage.setItem('coast-term-theme-dark', dark);
      }
      if (light != null && getTerminalThemeById(light) != null) {
        setLightThemeId(light);
        localStorage.setItem('coast-term-theme-light', light);
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const activeId = appTheme === 'dark' ? darkThemeId : lightThemeId;
  const activeTheme = getTerminalThemeById(activeId) ?? getTerminalThemeById(appTheme === 'dark' ? DEFAULT_DARK : DEFAULT_LIGHT)!;

  const setTerminalTheme = useCallback(
    (id: string) => {
      const def = getTerminalThemeById(id);
      if (def == null) return;
      if (def.mode === 'dark') {
        setDarkThemeId(id);
        localStorage.setItem('coast-term-theme-dark', id);
        void api.setSetting('terminal_theme_dark', id);
      } else {
        setLightThemeId(id);
        localStorage.setItem('coast-term-theme-light', id);
        void api.setSetting('terminal_theme_light', id);
      }
    },
    [],
  );

  return {
    activeTheme,
    appTheme,
    setTerminalTheme,
    themes: getTerminalThemesByMode(appTheme),
  };
}
