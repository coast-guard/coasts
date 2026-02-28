import { useMemo, type ReactNode } from 'react';

type DetectedFormat = 'json' | 'yaml' | 'plain';

function detectFormat(value: string): DetectedFormat {
  const trimmed = value.trim();
  if ((trimmed.startsWith('{') && trimmed.endsWith('}')) || (trimmed.startsWith('[') && trimmed.endsWith(']'))) {
    try { JSON.parse(trimmed); return 'json'; } catch { /* not json */ }
  }
  if (/^[\w.-]+\s*:/m.test(trimmed) && !trimmed.includes('{')) {
    return 'yaml';
  }
  return 'plain';
}

function formatValue(value: string, format: DetectedFormat): string {
  if (format === 'json') {
    try { return JSON.stringify(JSON.parse(value.trim()), null, 2); } catch { /* fall through */ }
  }
  return value;
}

function highlightJson(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const re = /("(?:[^"\\]|\\.)*")\s*:|("(?:[^"\\]|\\.)*")|(true|false|null)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = re.exec(text)) !== null) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index));
    }
    if (match[1] != null) {
      nodes.push(<span key={match.index} className="text-purple-400">{match[1]}</span>);
      nodes.push(':');
    } else if (match[2] != null) {
      nodes.push(<span key={match.index} className="text-emerald-400">{match[2]}</span>);
    } else if (match[3] != null) {
      nodes.push(<span key={match.index} className="text-amber-400">{match[3]}</span>);
    } else if (match[4] != null) {
      nodes.push(<span key={match.index} className="text-sky-400">{match[4]}</span>);
    }
    lastIndex = match.index + match[0].length;
  }

  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }
  return nodes;
}

function highlightYaml(text: string): ReactNode[] {
  return text.split('\n').flatMap((line, i, arr) => {
    const parts: ReactNode[] = [];
    const keyMatch = line.match(/^(\s*)([\w.-]+)(\s*:\s*)(.*)/);
    if (keyMatch != null) {
      const indent = keyMatch[1] ?? '';
      const key = keyMatch[2] ?? '';
      const colon = keyMatch[3] ?? '';
      const val = keyMatch[4] ?? '';
      parts.push(indent);
      parts.push(<span key={`k${i}`} className="text-purple-400">{key}</span>);
      parts.push(colon);
      if (val === 'true' || val === 'false' || val === 'null' || val === '~') {
        parts.push(<span key={`v${i}`} className="text-amber-400">{val}</span>);
      } else if (/^-?\d+(\.\d+)?$/.test(val)) {
        parts.push(<span key={`v${i}`} className="text-sky-400">{val}</span>);
      } else {
        parts.push(<span key={`v${i}`} className="text-emerald-400">{val}</span>);
      }
    } else if (line.trimStart().startsWith('#')) {
      parts.push(<span key={`c${i}`} className="text-slate-500 italic">{line}</span>);
    } else if (line.trimStart().startsWith('- ')) {
      const indent = line.match(/^(\s*)/)?.[1] ?? '';
      parts.push(indent);
      parts.push(<span key={`d${i}`} className="text-slate-400">- </span>);
      parts.push(<span key={`v${i}`} className="text-emerald-400">{line.trimStart().slice(2)}</span>);
    } else {
      parts.push(line);
    }
    if (i < arr.length - 1) parts.push('\n');
    return parts;
  });
}

export default function HighlightedValue({ value }: { readonly value: string }) {
  const format = detectFormat(value);
  const formatted = formatValue(value, format);

  const content = useMemo(() => {
    if (format === 'json') return highlightJson(formatted);
    if (format === 'yaml') return highlightYaml(formatted);
    return [formatted];
  }, [formatted, format]);

  return (
    <div className="relative">
      {format !== 'plain' && (
        <span className="absolute top-2 right-2 px-1.5 py-0.5 rounded text-[9px] font-semibold uppercase tracking-wider bg-white/10 text-slate-400 border border-white/5">
          {format}
        </span>
      )}
      <pre className="text-xs font-mono bg-slate-800/60 dark:bg-slate-900/60 text-slate-200 p-3 rounded-lg border border-[var(--border)] overflow-x-auto whitespace-pre-wrap break-all select-all max-h-[60vh] overflow-y-auto">
        {content}
      </pre>
    </div>
  );
}
