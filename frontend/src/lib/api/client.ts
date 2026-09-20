import type { ApiErrorBody } from './types'

const API_URL = (import.meta.env.VITE_API_URL as string | undefined)?.replace(/\/$/, '') ?? '/api'
const REQUEST_TIMEOUT_MS = 35_000

export class ApiError extends Error {
  status: number
  body: ApiErrorBody
  constructor(status: number, body: ApiErrorBody) {
    super(body.message)
    this.name = 'ApiError'
    this.status = status
    this.body = body
  }
}

export interface RequestOptions extends Omit<RequestInit, 'body'> { body?: unknown; accessToken?: string }

function combinedSignal(external?: AbortSignal | null) {
  const controller = new AbortController()
  const timeout = window.setTimeout(() => controller.abort(new DOMException('Request timed out', 'TimeoutError')), REQUEST_TIMEOUT_MS)
  const abortFromExternal = () => controller.abort(external?.reason)
  if (external?.aborted) abortFromExternal()
  else external?.addEventListener('abort', abortFromExternal, { once: true })
  return {
    signal: controller.signal,
    cleanup: () => {
      window.clearTimeout(timeout)
      external?.removeEventListener('abort', abortFromExternal)
    }
  }
}

export async function apiRequest<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers = new Headers(options.headers)
  headers.set('Accept', 'application/json')
  if (options.body !== undefined) headers.set('Content-Type', 'application/json')
  if (options.accessToken) headers.set('Authorization', `Bearer ${options.accessToken}`)
  const request = combinedSignal(options.signal)
  try {
    const response = await fetch(`${API_URL}${path}`, {
      ...options,
      signal: request.signal,
      credentials: 'include',
      headers,
      body: options.body === undefined ? undefined : JSON.stringify(options.body)
    })
    const contentType = response.headers.get('content-type') ?? ''
    const raw = await response.text()
    let payload: unknown = raw
    if (contentType.includes('application/json') && raw) {
      try { payload = JSON.parse(raw) }
      catch { throw new Error('CS Mailer returned an invalid JSON response.') }
    }
    if (!response.ok) {
      if (response.status === 401 && !path.startsWith('/v1/auth/')) window.dispatchEvent(new Event('mailer:session-expired'))
      const body: ApiErrorBody = typeof payload === 'string'
        ? { code: 'request_failed', message: payload || response.statusText }
        : payload && typeof payload === 'object'
          ? payload as ApiErrorBody
          : { code: 'request_failed', message: response.statusText || 'Request failed' }
      throw new ApiError(response.status, body)
    }
    return payload as T
  } catch (error) {
    if (request.signal.reason instanceof DOMException && request.signal.reason.name === 'TimeoutError') throw new Error('The request timed out. Check your connection and retry.')
    throw error
  } finally {
    request.cleanup()
  }
}

export const api = {
  get: <T>(path: string, options?: RequestOptions) => apiRequest<T>(path, { ...options, method: 'GET' }),
  post: <T>(path: string, body?: unknown, options?: RequestOptions) => apiRequest<T>(path, { ...options, method: 'POST', body }),
  patch: <T>(path: string, body?: unknown, options?: RequestOptions) => apiRequest<T>(path, { ...options, method: 'PATCH', body }),
  delete: <T>(path: string, options?: RequestOptions) => apiRequest<T>(path, { ...options, method: 'DELETE' })
}
