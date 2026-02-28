import type { ReactNode } from 'react';
import { TIMESTAMP_RE, LEVEL_RE, LEVEL_STYLES, highlightText } from '../lib/log-rendering';

export function renderHostServiceLogLine(text: string, idx: number, highlight?: RegExp): ReactNode {
  let rest = text;
  const elements: ReactNode[] = [];

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
