import type { ErrorResponse } from '../types/api';

const BASE = '/api/v1';

export class ApiError extends Error {
  constructor(
    message: string,
    public readonly status: number,
    public readonly body: ErrorResponse,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

async function parseJson<T>(response: Response): Promise<T> {
  const body: unknown = await response.json();
  if (!response.ok) {
    const err = body as ErrorResponse;
    throw new ApiError(err.error ?? `HTTP ${response.status}`, response.status, err);
  }
  return body as T;
}

export async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`);
  return parseJson<T>(res);
}

export async function post<TReq, TRes>(path: string, body: TReq): Promise<TRes> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  return parseJson<TRes>(res);
}

export async function del(path: string): Promise<void> {
  await fetch(`${BASE}${path}`, { method: 'DELETE' });
}

/** Fire-and-forget POST. Never throws, never blocks UI. */
export function beacon(path: string, body: unknown): void {
  fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  }).catch(() => {});
}

export { BASE };
