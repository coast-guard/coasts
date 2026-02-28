type SSEResult<TComplete> = { complete?: TComplete; error?: { error: string } };

export async function consumeSSE<TProgress, TComplete>(
  url: string,
  body: unknown,
  onProgress?: (event: TProgress) => void,
): Promise<SSEResult<TComplete>> {
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Accept: 'text/event-stream' },
    body: JSON.stringify(body),
  });

  if (!res.ok) {
    const text = await res.text().catch(() => 'unknown error');
    throw new Error(text);
  }

  const reader = res.body?.getReader();
  if (!reader) throw new Error('No response body');

  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() ?? '';
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i] ?? '';
      if (line.startsWith('event: progress') && onProgress) {
        const dataLine = lines[i + 1];
        if (dataLine?.startsWith('data: ')) {
          try { onProgress(JSON.parse(dataLine.slice(6)) as TProgress); } catch { /* ignore */ }
        }
      }
      if (line.startsWith('event: complete')) {
        const dataLine = lines[i + 1];
        if (dataLine?.startsWith('data: ')) {
          return { complete: JSON.parse(dataLine.slice(6)) as TComplete };
        }
      }
      if (line.startsWith('event: error')) {
        const dataLine = lines[i + 1];
        if (dataLine?.startsWith('data: ')) {
          return { error: JSON.parse(dataLine.slice(6)) as { error: string } };
        }
      }
    }
  }

  return {};
}
