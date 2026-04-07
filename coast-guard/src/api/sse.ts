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
  let currentEvent: string | null = null;

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() ?? '';
    for (const line of lines) {
      if (line.startsWith('event: ')) {
        currentEvent = line.slice(7).trim();
      } else if (line.startsWith('data: ') && currentEvent) {
        const payload = line.slice(6);
        if (currentEvent === 'progress' && onProgress) {
          try { onProgress(JSON.parse(payload) as TProgress); } catch { /* ignore */ }
        } else if (currentEvent === 'complete') {
          return { complete: JSON.parse(payload) as TComplete };
        } else if (currentEvent === 'error') {
          return { error: JSON.parse(payload) as { error: string } };
        }
        currentEvent = null;
      } else if (line === '') {
        currentEvent = null;
      }
    }
  }

  return { error: { error: 'Stream ended without a response' } };
}
