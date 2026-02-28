import type { ReactNode } from 'react';

export const ANSI_MAP: Record<string, string> = {
  '30': 'color:var(--text-subtle)', '31': 'color:#ef4444', '32': 'color:#22c55e',
  '33': 'color:#eab308', '34': 'color:#3b82f6', '35': 'color:#a855f7',
  '36': 'color:#06b6d4', '37': 'color:var(--text)', '90': 'color:var(--text-subtle)',
  '91': 'color:#f87171', '92': 'color:#4ade80', '93': 'color:#facc15',
  '94': 'color:#60a5fa', '95': 'color:#c084fc', '96': 'color:#22d3ee',
  '97': 'color:var(--text)', '1': 'font-weight:700', '2': 'opacity:0.6',
  '3': 'font-style:italic', '4': 'text-decoration:underline',
};

function parseCss(css: string): React.CSSProperties {
  const obj: Record<string, string> = {};
  for (const pair of css.split(';')) {
    const [k, v] = pair.split(':');
    if (k && v) obj[k.trim().replace(/-([a-z])/g, (_, c: string) => c.toUpperCase())] = v.trim();
  }
  return obj as React.CSSProperties;
}

export function parseAnsi(text: string): ReactNode[] {
  const parts: ReactNode[] = [];
  // eslint-disable-next-line no-control-regex
  const regex = /\x1b\[([0-9;]*)m/g;
  let lastIndex = 0;
  let activeStyles: string[] = [];
  let match: RegExpExecArray | null;

  while ((match = regex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      const chunk = text.slice(lastIndex, match.index);
      parts.push(activeStyles.length > 0
        ? <span key={lastIndex} style={parseCss(activeStyles.join(';'))}>{chunk}</span>
        : chunk);
    }
    for (const code of match[1]!.split(';')) {
      if (code === '0' || code === '') { activeStyles = []; }
      else { const s = ANSI_MAP[code]; if (s) activeStyles.push(s); }
    }
    lastIndex = match.index + match[0].length;
  }
  if (lastIndex < text.length) {
    const chunk = text.slice(lastIndex);
    parts.push(activeStyles.length > 0
      ? <span key={lastIndex} style={parseCss(activeStyles.join(';'))}>{chunk}</span>
      : chunk);
  }
  return parts;
}

export const LEVEL_STYLES: Record<string, string> = {
  ERROR: 'text-rose-500 font-semibold', WARN: 'text-amber-500 font-semibold',
  WARNING: 'text-amber-500 font-semibold', INFO: 'text-blue-400',
  DEBUG: 'text-slate-400', TRACE: 'text-slate-500',
};

export const TIMESTAMP_RE = /^(\d{4}[-/]\d{2}[-/]\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?)\s*/;
export const LEVEL_RE = /\b(ERROR|WARN|WARNING|INFO|DEBUG|TRACE)\b/;

export function highlightText(text: string, lineIdx: number, prefix: string, highlight?: RegExp): ReactNode[] {
  if (highlight == null) return parseAnsi(text);

  const parts: ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  const re = new RegExp(highlight.source, highlight.flags.includes('g') ? highlight.flags : highlight.flags + 'g');

  while ((match = re.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push(...parseAnsi(text.slice(lastIndex, match.index)));
    }
    parts.push(
      <mark key={`h${lineIdx}${prefix}${match.index}`} className="bg-amber-400/30 text-amber-200 rounded px-0.5">
        {match[0]}
      </mark>,
    );
    lastIndex = match.index + match[0].length;
    if (match[0].length === 0) { re.lastIndex++; }
  }
  if (lastIndex < text.length) parts.push(...parseAnsi(text.slice(lastIndex)));
  return parts;
}
