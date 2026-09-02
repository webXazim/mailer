import { ReactNode, useCallback, useEffect, useRef, useState } from 'react'
import { RefreshCw, X } from 'lucide-react'
import { api } from '../../lib/api/client'
export type Envelope<T> = { data: T; hasMore?: boolean; nextOffset?: number | null }
export type Environment = 'test' | 'production'
export type Session = { user: { id: string; name: string; email: string; role: string }; workspace: { id: string; name: string; production_enabled: boolean; usage: { sent: number; limit: number } } }
export type Domain = { id: string; domain: string; status: string; dns_automation: string[]; records: { record_type: string; name: string; value: string; required: boolean; status: string; last_checked_at?: string }[] }
export type Key = { id: string; name: string; prefix: string; environment: Environment; scopes: string[] }
export type Endpoint = { id: string; url: string; environment: Environment; subscriptions: string[]; enabled: boolean }
export type Delivery = { id: string; status: string; attempts: number; eventType: string; lastError?: string; createdAt: string }
export type Suppression = { id: string; address: string; reason: string; createdAt: string }
export type Email = { id: string; environment: Environment; from: string; subject: string; status: string; acceptedAt: string; lastError?: string; recipients: { address: string; type: string; status: string }[]; metadata: unknown; events?: unknown[]; contentAvailable?: boolean; content?: { text?: string; html?: string; attachments?: unknown[] } }
export const errorText = (error: unknown) => error instanceof Error ? error.message : 'Request failed. Please retry.'
export const date = (value: string) => new Date(value).toLocaleString()
export function useResource<T>(path: string | null, poll = false) {
  const [result, setResult] = useState<Envelope<T>>()
  const [error, setError] = useState(''), [loading, setLoading] = useState(true), [revision, setRevision] = useState(0)
  const previousPath = useRef(path)
  const reload = useCallback(() => setRevision(value => value + 1), [])
  useEffect(() => {
    const pathChanged = previousPath.current !== path
    previousPath.current = path
    if (!path) { setResult(undefined); setLoading(false); return }
    const controller = new AbortController()
    if (pathChanged) setResult(undefined)
    setLoading(pathChanged || !result); setError('')
    api.get<Envelope<T>>(path, { signal: controller.signal }).then(value => { if (!controller.signal.aborted) setResult(value) }).catch(error => {
      if (!controller.signal.aborted) setError(errorText(error))
    }).finally(() => { if (!controller.signal.aborted) setLoading(false) })
    return () => controller.abort()
  }, [path, revision])
  useEffect(() => {
    if (!poll) return
    const timer = setInterval(() => { if (!document.hidden) reload() }, 15000)
    return () => clearInterval(timer)
  }, [poll, reload])
  return { result, error, loading, reload }
}
export function useAction() {
  const [busy, setBusy] = useState(false), [error, setError] = useState('')
  async function run(action: () => Promise<void>) {
    if (busy) return
    setBusy(true); setError('')
    try { await action() } catch (error) { setError(errorText(error)) } finally { setBusy(false) }
  }
  return { busy, error, run }
}
export function ErrorNotice({ error }: { error?: string }) { return error ? <p className="live-error" role="alert">{error}</p> : null }
export function Field({ label, children }: { label: string; children: ReactNode }) { return <label className="field"><span>{label}</span>{children}</label> }
export function Panel({ title, children, action }: { title: string; children: ReactNode; action?: ReactNode }) { return <section className="panel live-panel"><div className="panel__header"><h2>{title}</h2>{action}</div><div className="live-panel__body">{children}</div></section> }
export function Submit({ busy, children }: { busy: boolean; children: ReactNode }) { return <button className="button button--primary" disabled={busy} type="submit">{busy ? 'Working…' : children}</button> }
export function Refresh({ reload }: { reload: () => void }) { return <button className="button button--secondary" onClick={reload}><RefreshCw size={14} />Refresh</button> }
export function Badge({ value }: { value: string }) { return <span className={`live-badge live-badge--${value}`}>{value}</span> }
export function Secret({ value, close }: { value: string; close: () => void }) {
  const action = useAction()
  return <div className="modal-backdrop"><section className="modal" role="dialog" aria-modal="true" aria-labelledby="secret-title"><div className="modal__header"><h2 id="secret-title">Save this secret now</h2><button className="icon-button" aria-label="Close secret" onClick={close}><X size={16} /></button></div><div className="live-panel__body"><p>Shown once. Keep it in your server environment, never in browser code.</p><textarea aria-label="Generated secret" readOnly rows={3} value={value} onFocus={e => e.target.select()} /><ErrorNotice error={action.error} /><button className="button button--primary" onClick={() => action.run(async () => navigator.clipboard.writeText(value))}>Copy secret</button></div></section></div>
}
