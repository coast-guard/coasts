import type { ReactNode } from 'react';
import { TIMESTAMP_RE, LEVEL_RE, LEVEL_STYLES, highlightText } from '../lib/log-rendering';

const SERVICE_COLORS = [
  '#3b82f6', '#8b5cf6', '#06b6d4', '#f97316', '#ec4899',
  '#10b981', '#f59e0b', '#6366f1', '#14b8a6', '#e11d48',
];

export function getServiceColor(service: string): string {
  let hash = 0;
  for (let i = 0; i < service.length; i++) {
    hash = ((hash << 5) - hash + service.charCodeAt(i)) | 0;
  }
  return SERVICE_COLORS[Math.abs(hash) % SERVICE_COLORS.length]!;
}

export interface ParsedLine {
  readonly raw: string;
  readonly service: string | null;
  readonly text: string;
}

const COMPOSE_SERVICE_RE = /^([a-zA-Z0-9_.-]+)\s+\|\s*/;

export function parseLine(raw: string): ParsedLine {
  const m = COMPOSE_SERVICE_RE.exec(raw);
  return m != null
    ? { raw, service: m[1]!, text: raw.slice(m[0].length) }
    : { raw, service: null, text: raw };
}

export function renderInstanceLogLine(parsed: ParsedLine, idx: number, highlight?: RegExp): ReactNode {
  let rest = parsed.text;
  const elements: ReactNode[] = [];

  if (parsed.service != null) {
    const c = getServiceColor(parsed.service);
    elements.push(
      <span key={`s${idx}`} className="inline-block mr-2 px-1.5 py-0.5 rounded text-[10px] font-semibold" style={{ background: c + '20', color: c }}>
        {parsed.service}
      </span>,
    );
  }

  const tsMatch = TIMESTAMP_RE.exec(rest);
  if (tsMatch != null) {
    elements.push(<span key={`t${idx}`} className="text-subtle-ui">{tsMatch[1]} </span>);
    rest = rest.slice(tsMatch[0].length);
  }

  const levelMatch = LEVEL_RE.exec(rest);
  if (levelMatch != null) {
    const pre = rest.slice(0, levelMatch.index);
    const level = levelMatch[1]!;
    const post = rest.slice(levelMatch.index + level.length);
    if (pre.length > 0) elements.push(...highlightText(pre, idx, 'a', highlight));
    elements.push(<span key={`l${idx}`} className={LEVEL_STYLES[level] ?? ''}>{level}</span>);
    elements.push(...highlightText(post, idx, 'b', highlight));
  } else {
    elements.push(...highlightText(rest, idx, 'c', highlight));
  }

  return <div key={idx} className="leading-5 hover:bg-white/5 px-1 -mx-1 rounded">{elements}</div>;
}
