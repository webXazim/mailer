import { ReactNode, useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { Check, Copy, LoaderCircle, RefreshCw, X } from 'lucide-react'
import { api } from '../../lib/api/client'

export type Envelope<T> = { data: T; hasMore?: boolean; nextOffset?: number | null }
export type Environment = 'test' | 'production'
export type Session = {
  user: { id: string; name: string; email: string; role: string }
  workspace: {
    id: string
    name: string
    slug?: string
    plan?: string
    production_enabled: boolean
    sending_paused: boolean
    sending_pause_reason?: string | null
    usage: { sent: number; limit: number }
  }
}
export type DomainRecord = { record_type: string; name: string; value: string; required: boolean; status: string; last_checked_at?: string | null }
export type Domain = { id: string; domain: string; status: string; provider: string; verified_at?: string | null; created_at?: string; dns_automation: string[]; records: DomainRecord[] }
export type Key = { id: string; name: string; prefix: string; environment: Environment; scopes: string[]; expiresAt?: string | null; lastUsedAt?: string | null; createdAt?: string; expires_at?: string | null; last_used_at?: string | null; created_at?: string }
export type Endpoint = { id: string; url: string; environment: Environment; subscriptions: string[]; enabled: boolean; failureCount?: number; lastSuccessAt?: string | null; lastFailureAt?: string | null; createdAt?: string; updatedAt?: string }
export type Delivery = { id: string; status: string; attempts: number; eventType: string; recipient?: string | null; lastError?: string; createdAt: string; occurredAt?: string; nextAttemptAt?: string; completedAt?: string | null }
export type WebhookAttempt = { attemptNumber: number; statusCode?: number | null; error?: string | null; nextRetryAt?: string | null; deliveredAt?: string | null; createdAt: string }
export type Suppression = { id: string; address: string; reason: string; createdAt: string }
export type EmailRecipient = { address: string; type: string; status: string }
export type EmailEvent = { id?: string; type?: string; recipient?: string | null; occurredAt?: string; data?: unknown }
export type ProviderAttempt = { id?: string; provider?: string; attemptNumber?: number; status?: string; providerMessageId?: string | null; error?: string | null; startedAt?: string; completedAt?: string | null }
export type Email = {
  id: string
  environment: Environment
  deliveryProvider?: string
  from: string
  subject: string
  status: string
  acceptedAt: string
  sentAt?: string | null
  completedAt?: string | null
  lastError?: string
  providerMessageId?: string | null
  providerAttempts?: ProviderAttempt[]
  recipients: EmailRecipient[]
  metadata: unknown
  events?: EmailEvent[]
  contentAvailable?: boolean
  content?: { text?: string; html?: string; attachments?: Array<{ filename?: string; content_type?: string; size?: number }> }
}

export const errorText = (error: unknown) => error instanceof Error ? error.message : 'Request failed. Please retry.'
export const date = (value?: string | null) => value ? new Date(value).toLocaleString() : '—'
export const relativeDate = (value?: string | null) => {
  if (!value) return '—'
  const diff = Date.now() - new Date(value).getTime()
  const future = diff < 0
  const seconds = Math.max(0, Math.round(Math.abs(diff) / 1000))
  const suffix = future ? 'from now' : 'ago'
  if (seconds < 8) return 'just now'
  if (seconds < 60) return `${seconds}s ${suffix}`
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m ${suffix}`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h ${suffix}`
  const days = Math.round(hours / 24)
  if (days < 7) return `${days}d ${suffix}`
  return date(value)
}
export const statusTone = (value: string) => {
  const status = value.toLowerCase()
  if (['delivered', 'verified', 'active', 'enabled', 'success', 'sent', 'complete', 'completed'].includes(status)) return 'success'
  if (['failed', 'bounced', 'complaint', 'complained', 'rejected', 'disabled', 'error'].includes(status)) return 'danger'
  if (['pending', 'queued', 'processing', 'retrying', 'verifying'].includes(status)) return 'warning'
  return 'neutral'
}

/**
 * Shared resource cache for the console. It is intentionally independent of route
 * rendering: a resource refresh updates only subscribers of that resource, never the
 * browser document or the complete application shell.
 */
type CacheState<T> = {
  result?: Envelope<T>
  error: string
  loading: boolean
  refreshing: boolean
  updatedAt: number
}
type Inflight = { promise: Promise<void>; controller: AbortController; epoch: number }
export type PollSetting<T> = boolean | number | ((result?: Envelope<T>) => number | false)

const EMPTY: CacheState<never> = { error: '', loading: false, refreshing: false, updatedAt: 0 }
const cache = new Map<string, CacheState<unknown>>()
const listeners = new Map<string, Set<() => void>>()
const inflight = new Map<string, Inflight>()
const epochs = new Map<string, number>()

function epoch(path: string) { return epochs.get(path) ?? 0 }
function bumpEpoch(path: string) { epochs.set(path, epoch(path) + 1) }
function ensure<T>(path: string): CacheState<T> {
  if (!cache.has(path)) cache.set(path, { error: '', loading: true, refreshing: false, updatedAt: 0 })
  return cache.get(path)! as CacheState<T>
}
function publish<T>(path: string, state: CacheState<T>) {
  cache.set(path, state as CacheState<unknown>)
  listeners.get(path)?.forEach(listener => listener())
}
function isAbort(error: unknown) { return error instanceof DOMException && error.name === 'AbortError' }

function cancelInflight(path: string) {
  const running = inflight.get(path)
  if (!running) return
  bumpEpoch(path)
  running.controller.abort()
  inflight.delete(path)
}

async function fetchResource<T>(path: string, restart = false) {
  const running = inflight.get(path)
  if (running && !restart) return running.promise
  if (running && restart) {
    bumpEpoch(path)
    running.controller.abort()
    inflight.delete(path)
  }

  const requestEpoch = epoch(path)
  const controller = new AbortController()
  const previous = ensure<T>(path)
  publish(path, { ...previous, loading: !previous.result, refreshing: Boolean(previous.result), error: '' })

  let task!: Promise<void>
  task = api.get<Envelope<T>>(path, { signal: controller.signal })
    .then(result => {
      if (epoch(path) !== requestEpoch || controller.signal.aborted) return
      publish(path, { result, error: '', loading: false, refreshing: false, updatedAt: Date.now() })
    })
    .catch(error => {
      if (epoch(path) !== requestEpoch || controller.signal.aborted || isAbort(error)) return
      const current = ensure<T>(path)
      publish(path, { ...current, error: errorText(error), loading: false, refreshing: false, updatedAt: Date.now() })
    })
    .finally(() => {
      if (inflight.get(path)?.promise === task) inflight.delete(path)
    })
  inflight.set(path, { promise: task, controller, epoch: requestEpoch })
  return task
}

function resolvePoll<T>(setting: PollSetting<T>, result?: Envelope<T>) {
  if (typeof setting === 'function') return setting(result)
  if (setting === true) return 15_000
  if (typeof setting === 'number' && setting > 0) return setting
  return false
}

export const queryClient = {
  async invalidate(prefixes: string | string[]) {
    const list = Array.isArray(prefixes) ? prefixes : [prefixes]
    const paths = [...cache.keys()].filter(path => list.some(prefix => path.startsWith(prefix)))
    await Promise.all(paths.map(path => fetchResource(path, true)))
  },
  remove(prefix: string) {
    for (const path of new Set([...cache.keys(), ...inflight.keys()])) {
      if (!path.startsWith(prefix)) continue
      bumpEpoch(path)
      inflight.get(path)?.controller.abort()
      inflight.delete(path)
      cache.delete(path)
      listeners.get(path)?.forEach(listener => listener())
    }
  },
  set<T>(path: string, result: Envelope<T>) {
    cancelInflight(path)
    publish(path, { result, error: '', loading: false, refreshing: false, updatedAt: Date.now() })
  },
  update<T>(path: string, updater: (current: Envelope<T> | undefined) => Envelope<T> | undefined) {
    cancelInflight(path)
    const previous = ensure<T>(path)
    const result = updater(previous.result)
    publish(path, { ...previous, result, error: result ? '' : previous.error, loading: result ? false : previous.loading, refreshing: false, updatedAt: Date.now() })
  }
}

export function useResource<T>(path: string | null, poll: PollSetting<T> = false) {
  const subscribe = useCallback((listener: () => void) => {
    if (!path) return () => undefined
    const bucket = listeners.get(path) ?? new Set<() => void>()
    bucket.add(listener)
    listeners.set(path, bucket)
    return () => {
      bucket.delete(listener)
      if (!bucket.size) listeners.delete(path)
    }
  }, [path])
  const getSnapshot = useCallback(() => path ? ensure<T>(path) : EMPTY as CacheState<T>, [path])
  const state = useSyncExternalStore(subscribe, getSnapshot, getSnapshot)
  const reload = useCallback(() => path ? fetchResource<T>(path, true) : Promise.resolve(), [path])

  useEffect(() => {
    if (!path) return
    let cancelled = false
    let timer: number | undefined

    const getInterval = () => resolvePoll(poll, ensure<T>(path).result)
    const initialInterval = getInterval()
    const staleAfter = initialInterval || 30_000
    const current = ensure<T>(path)
    if ((!current.result || Date.now() - current.updatedAt > staleAfter) && navigator.onLine) void fetchResource<T>(path)

    const schedule = () => {
      const interval = getInterval()
      if (!interval || cancelled) return
      timer = window.setTimeout(async () => {
        if (!cancelled && !document.hidden && navigator.onLine) {
          const next = ensure<T>(path)
          if (!next.updatedAt || Date.now() - next.updatedAt >= Math.max(750, interval * .8)) await fetchResource<T>(path)
        }
        schedule()
      }, interval)
    }
    schedule()

    const refreshIfVisible = () => {
      if (document.hidden || !navigator.onLine) return
      const interval = getInterval() || 30_000
      const next = ensure<T>(path)
      if (!next.updatedAt || Date.now() - next.updatedAt > Math.min(interval, 15_000)) void fetchResource<T>(path)
    }
    const online = () => { if (!document.hidden) void fetchResource<T>(path) }
    window.addEventListener('focus', refreshIfVisible)
    window.addEventListener('online', online)
    document.addEventListener('visibilitychange', refreshIfVisible)
    return () => {
      cancelled = true
      if (timer) window.clearTimeout(timer)
      window.removeEventListener('focus', refreshIfVisible)
      window.removeEventListener('online', online)
      document.removeEventListener('visibilitychange', refreshIfVisible)
    }
  }, [path, poll])
  return { ...state, reload }
}

export function useOnlineStatus() {
  const [online, setOnline] = useState(() => navigator.onLine)
  useEffect(() => {
    const yes = () => setOnline(true)
    const no = () => setOnline(false)
    window.addEventListener('online', yes)
    window.addEventListener('offline', no)
    return () => { window.removeEventListener('online', yes); window.removeEventListener('offline', no) }
  }, [])
  return online
}

export function useDialogLifecycle(close: () => void) {
  const dialogRef = useRef<HTMLElement>(null)
  const closeRef = useRef(close)
  closeRef.current = close
  useEffect(() => {
    const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    const focusableSelector = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
    const focusFirst = window.requestAnimationFrame(() => {
      const target = dialogRef.current?.querySelector<HTMLElement>('[autofocus]') ?? dialogRef.current?.querySelector<HTMLElement>(focusableSelector) ?? dialogRef.current
      target?.focus()
    })
    const keydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); closeRef.current(); return }
      if (event.key !== 'Tab' || !dialogRef.current) return
      const focusable = [...dialogRef.current.querySelectorAll<HTMLElement>(focusableSelector)].filter(element => element.offsetParent !== null)
      if (!focusable.length) { event.preventDefault(); dialogRef.current.focus(); return }
      const first = focusable[0], last = focusable[focusable.length - 1]
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus() }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus() }
    }
    document.addEventListener('keydown', keydown)
    return () => {
      window.cancelAnimationFrame(focusFirst)
      document.removeEventListener('keydown', keydown)
      document.body.style.overflow = previousOverflow
      if (previousFocus?.isConnected) previousFocus.focus()
    }
  }, [])
  return dialogRef
}

export function useAction() {
  const [busy, setBusy] = useState(false), [error, setError] = useState('')
  const busyRef = useRef(false)
  async function run(action: () => Promise<void>) {
    if (busyRef.current) return false
    busyRef.current = true
    setBusy(true)
    setError('')
    try { await action(); return true }
    catch (error) { setError(errorText(error)); return false }
    finally { busyRef.current = false; setBusy(false) }
  }
  return { busy, error, setError, run }
}

export function BrandLogo({ compact = false, className = '' }: { compact?: boolean; className?: string }) {
  return <span className={`cs-brand ${compact ? 'cs-brand--compact' : ''} ${className}`.trim()}>
    <span className="cs-brand__mark"><img src="/cs-mailer-logo.png" alt="" /></span>
    {!compact && <span className="cs-brand__copy"><strong>CS Mailer</strong><small>by CrescentSphere</small></span>}
  </span>
}
export function ErrorNotice({ error }: { error?: string }) { return error ? <p className="notice notice--danger" role="alert" aria-live="assertive">{error}</p> : null }
export function Notice({ children, tone = 'info' }: { children: ReactNode; tone?: 'info' | 'success' | 'warning' | 'danger' }) { return <p className={`notice notice--${tone}`} role="status" aria-live="polite">{children}</p> }
export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) { return <label className="field"><span className="field__label">{label}</span>{children}{hint && <small>{hint}</small>}</label> }
export function Panel({ title, eyebrow, children, action, className = '' }: { title: string; eyebrow?: string; children: ReactNode; action?: ReactNode; className?: string }) { return <section className={`panel ${className}`.trim()}><div className="panel__header"><div>{eyebrow && <p className="eyebrow">{eyebrow}</p>}<h2>{title}</h2></div>{action}</div><div className="panel__body">{children}</div></section> }
export function Submit({ busy, children, className = '' }: { busy: boolean; children: ReactNode; className?: string }) { return <button className={`button button--primary ${className}`.trim()} disabled={busy} aria-busy={busy} type="submit">{busy && <LoaderCircle className="spin" size={15} />}{children}</button> }
export function Refresh({ reload, refreshing = false, label = 'Refresh' }: { reload: () => void | Promise<void>; refreshing?: boolean; label?: string }) { return <button className="button button--ghost button--compact" onClick={() => void reload()} disabled={refreshing} aria-busy={refreshing}><RefreshCw className={refreshing ? 'spin' : ''} size={14} />{label}</button> }
export function Badge({ value, label }: { value: string; label?: string }) { return <span className={`status-badge status-badge--${statusTone(value)}`}><i />{label ?? value.replaceAll('_', ' ')}</span> }

async function copyText(value: string) {
  if (navigator.clipboard?.writeText) { await navigator.clipboard.writeText(value); return }
  const field = document.createElement('textarea')
  field.value = value
  field.style.position = 'fixed'
  field.style.opacity = '0'
  document.body.appendChild(field)
  field.select()
  document.execCommand('copy')
  field.remove()
}
export function CopyButton({ value, label = 'Copy' }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false), [copyError, setCopyError] = useState(false)
  async function copy() {
    try { await copyText(value); setCopied(true); setCopyError(false); window.setTimeout(() => setCopied(false), 1400) }
    catch { setCopyError(true); window.setTimeout(() => setCopyError(false), 1800) }
  }
  return <button type="button" className="icon-text-button" onClick={() => void copy()} aria-live="polite">{copied ? <Check size={14} /> : <Copy size={14} />}{copyError ? 'Copy failed' : copied ? 'Copied' : label}</button>
}
export function Secret({ value, close, title = 'Save this secret now' }: { value: string; close: () => void; title?: string }) {
  const dialogRef = useDialogLifecycle(close)
  return <div className="modal-backdrop" onMouseDown={e => { if (e.target === e.currentTarget) close() }}><section ref={dialogRef} tabIndex={-1} className="modal modal--secret" role="dialog" aria-modal="true" aria-labelledby="secret-title"><div className="modal__header"><div><p className="eyebrow">One-time credential</p><h2 id="secret-title">{title}</h2></div><button className="icon-button" aria-label="Close secret" onClick={close}><X size={18} /></button></div><div className="modal__body"><Notice tone="warning">This value is shown once. Store it in a server-side secret manager and never ship it to browser or mobile code.</Notice><div className="secret-box"><code>{value}</code><CopyButton value={value} label="Copy secret" /></div><button className="button button--primary" onClick={close}>I saved it</button></div></section></div>
}
export function EmptyState({ title, copy, children }: { title: string; copy: string; children?: ReactNode }) { return <div className="empty-state"><span className="empty-state__pulse" /><h3>{title}</h3><p>{copy}</p>{children}</div> }
export function SkeletonRows({ rows = 4 }: { rows?: number }) { return <div className="skeleton-list" aria-label="Loading" role="status" aria-live="polite">{Array.from({ length: rows }, (_, index) => <span key={index} />)}</div> }
