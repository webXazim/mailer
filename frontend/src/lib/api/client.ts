import type { ApiErrorBody } from './types'

const API_URL = (import.meta.env.VITE_API_URL as string | undefined)?.replace(/\/$/, '') ?? '/api'

export class ApiError extends Error {
  status: number
  body: ApiErrorBody
  constructor(status: number, body: ApiErrorBody) { super(body.message); this.name = 'ApiError'; this.status = status; this.body = body }
}

export interface RequestOptions extends Omit<RequestInit, 'body'> { body?: unknown; accessToken?: string }

export async function apiRequest<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers = new Headers(options.headers)
  headers.set('Accept', 'application/json')
  if (options.body !== undefined) headers.set('Content-Type', 'application/json')
  if (options.accessToken) headers.set('Authorization', `Bearer ${options.accessToken}`)
  const response = await fetch(`${API_URL}${path}`, { ...options, credentials: 'include', headers, body: options.body === undefined ? undefined : JSON.stringify(options.body) })
  const contentType = response.headers.get('content-type') ?? ''
  const payload = contentType.includes('application/json') ? await response.json() : await response.text()
  if (!response.ok) {
    const body: ApiErrorBody = typeof payload === 'string' ? { code: 'request_failed', message: payload || response.statusText } : payload
    throw new ApiError(response.status, body)
  }
  return payload as T
}

export const api = {
  get: <T>(path: string, options?: RequestOptions) => apiRequest<T>(path, { ...options, method: 'GET' }),
  post: <T>(path: string, body?: unknown, options?: RequestOptions) => apiRequest<T>(path, { ...options, method: 'POST', body }),
  patch: <T>(path: string, body?: unknown, options?: RequestOptions) => apiRequest<T>(path, { ...options, method: 'PATCH', body }),
  delete: <T>(path: string, options?: RequestOptions) => apiRequest<T>(path, { ...options, method: 'DELETE' })
}
