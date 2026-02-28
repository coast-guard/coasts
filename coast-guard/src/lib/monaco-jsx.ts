/**
 * Registers custom editor themes with Monaco.
 *
 * We intentionally do NOT replace Monaco's built-in TypeScript/JavaScript
 * tokenizer. The built-in tokenizer uses the TypeScript compiler's scanner
 * which properly understands the AST — it handles generics vs JSX, type
 * assertions, template literals, etc. correctly. A Monarch (regex) tokenizer
 * cannot distinguish `<div>` (JSX) from `<T>` (generic) or `a < b` (comparison).
 *
 * JSX coloring comes from two sources, both AST-aware:
 * 1. Monaco's built-in TS tokenizer (syntactic tokens with `.tsx` file extension)
 * 2. Semantic tokens from typescript-language-server via our LSP bridge
 */
import type { Monaco } from '@monaco-editor/react';

export interface EditorThemeInput {
  readonly id: string;
  readonly base: 'vs' | 'vs-dark';
  readonly rules: readonly { readonly token: string; readonly foreground?: string; readonly fontStyle?: string }[];
  readonly colors: Record<string, string>;
}

export function setupJsxSupport(monaco: Monaco, themes: readonly EditorThemeInput[]): void {
  for (const t of themes) {
    monaco.editor.defineTheme(t.id, {
      base: t.base,
      inherit: true,
      rules: t.rules.map((r) => ({ token: r.token, foreground: r.foreground, fontStyle: r.fontStyle })),
      colors: t.colors,
    });
  }

  // Monaco does not ship TOML highlighting by default in every bundle.
  // Register a lightweight TOML tokenizer once so Coastfile views are colored.
  if (!monaco.languages.getLanguages().some((lang: { id: string }) => lang.id === 'toml')) {
    monaco.languages.register({ id: 'toml' });
    monaco.languages.setMonarchTokensProvider('toml', {
      tokenizer: {
        root: [
          [/^\s*#.*/, 'comment'],
          [/^\s*\[\[.*\]\]\s*$/, 'type'],
          [/^\s*\[.*\]\s*$/, 'type'],
          [/^\s*[A-Za-z0-9_.-]+\s*=/, 'key'],
          [/"([^"\\]|\\.)*"/, 'string'],
          [/'[^']*'/, 'string'],
          [/\b(true|false)\b/, 'keyword'],
          [/\b\d{4}-\d{2}-\d{2}([Tt ][0-9:.+-Zz]+)?\b/, 'number'],
          [/\b[+-]?\d+(_\d+)*(\.\d+(_\d+)*)?([eE][+-]?\d+)?\b/, 'number'],
          [/[{}[\],]/, 'delimiter.bracket'],
        ],
      },
    });
    monaco.languages.setLanguageConfiguration('toml', {
      comments: { lineComment: '#' },
      brackets: [['[', ']'], ['{', '}']],
      autoClosingPairs: [
        { open: '"', close: '"' },
        { open: "'", close: "'" },
        { open: '[', close: ']' },
        { open: '{', close: '}' },
      ],
    });
  }
}
