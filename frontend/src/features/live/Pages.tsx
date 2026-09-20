import { FormEvent, ReactNode, useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import {
  Activity, AlertTriangle, ArrowRight, Check, ChevronDown, CircleDot, Cloud,
  Code2, Copy, ExternalLink, FileText, Globe2, KeyRound, Mail, MoreHorizontal,
  RefreshCw, RotateCw, Search, Send, ShieldBan, Sparkles, Trash2, Webhook, X
} from 'lucide-react'
import { api } from '../../lib/api/client'
import {
  Badge, CopyButton, date, Delivery, Domain, Email, EmptyState, Endpoint, Envelope,
  Environment, ErrorNotice, Field, Key, Notice, Panel, queryClient, Refresh,
  relativeDate, Secret, Session, SkeletonRows, Submit, Suppression, useAction,
  useDialogLifecycle, useResource, WebhookAttempt
} from './shared'

function EnvironmentPill({ environment }: { environment: Environment }) {
  return <span className={`environment-pill environment-pill--${environment}`}>{environment === 'test' ? 'TEST' : 'PRODUCTION'}</span>
}
function Metric({ label, value, copy, icon }: { label: string; value: ReactNode; copy: string; icon: ReactNode }) {
  return <article className="metric"><span className="metric__icon">{icon}</span><div><p>{label}</p><strong>{value}</strong><small>{copy}</small></div></article>
}
function recipientSummary(email: Email) {
  const addresses = email.recipients.map(item => item.address)
  if (!addresses.length) return 'No recipients'
  return addresses.length === 1 ? addresses[0] : `${addresses[0]} +${addresses.length - 1}`
}
function finalEmailStatus(status: string) {
  return ['delivered', 'bounced', 'complaint', 'complained', 'rejected', 'failed', 'rendering_failure'].includes(status.toLowerCase())
}

const emailDetailPoll = (result?: Envelope<Email>) => result && finalEmailStatus(result.data.status) ? false : 2_500

function DrawerSurface({ close, labelledBy, wide = false, children }: { close: () => void; labelledBy: string; wide?: boolean; children: ReactNode }) {
  const ref = useDialogLifecycle(close)
  return <div className="drawer-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) close() }}><aside ref={ref} tabIndex={-1} className={`drawer ${wide ? 'drawer--wide' : ''}`.trim()} role="dialog" aria-modal="true" aria-labelledby={labelledBy}>{children}</aside></div>
}

export function Overview({ environment, session, admin, send, go }: { environment: Environment; session: Session; admin: boolean; send: () => void; go: (path: string) => void }) {
  const emails = useResource<Email[]>(`/v1/emails?environment=${environment}&limit=12&offset=0`, 5_000)
  const domains = useResource<Domain[]>('/v1/domains', 30_000)
  const webhooks = useResource<Endpoint[]>(admin ? '/v1/webhooks' : null, 30_000)
  const recent = emails.result?.data ?? []
  const delivered = recent.filter(item => item.status === 'delivered').length
  const verifiedDomains = (domains.result?.data ?? []).filter(item => item.status === 'verified').length
  const activeWebhooks = (webhooks.result?.data ?? []).filter(item => item.enabled && item.environment === environment).length
  const usagePercent = session.workspace.usage.limit ? Math.min(100, Math.round((session.workspace.usage.sent / session.workspace.usage.limit) * 100)) : 0

  return <div className="page-stack">
    <section className="overview-hero">
      <div><div className="overview-hero__meta"><EnvironmentPill environment={environment} /><span className="live-indicator"><i />Live workspace data</span></div><h2>Mail operations at a glance.</h2><p>Watch message activity, sender readiness, and workspace usage without refreshing the page.</p></div>
      {admin && <button className="button button--primary" onClick={send}><Send size={15} />Send email</button>}
    </section>

    <section className="metrics-grid">
      <Metric label="Monthly submissions" value={session.workspace.usage.sent.toLocaleString()} copy={`${session.workspace.usage.limit.toLocaleString()} accepted-message limit`} icon={<Activity size={18} />} />
      <Metric label="Recent delivered" value={recent.length ? `${delivered}/${recent.length}` : '—'} copy="From the latest visible messages" icon={<Mail size={18} />} />
      <Metric label="Verified domains" value={verifiedDomains} copy={verifiedDomains ? 'Ready sender identities' : 'Test mode is still available'} icon={<Globe2 size={18} />} />
      <Metric label="Active webhooks" value={admin ? activeWebhooks : '—'} copy={admin ? `${environment} endpoints enabled` : 'Administrator visibility only'} icon={<Webhook size={18} />} />
    </section>

    <section className="overview-grid">
      <Panel title="Workspace usage" eyebrow="Current month" className="usage-panel">
        <div className="usage-number"><strong>{usagePercent}%</strong><span>{session.workspace.usage.sent.toLocaleString()} / {session.workspace.usage.limit.toLocaleString()}</span></div>
        <div className="usage-track" aria-label={`${usagePercent}% of monthly submission limit used`}><span style={{ width: `${usagePercent}%` }} /></div>
        <p className="muted">The monthly counter includes accepted test submissions as well as production submissions.</p>
      </Panel>
      <Panel title="Production readiness" eyebrow="Integration path">
        <div className="readiness-list">{admin && <button onClick={() => go('/api-keys')}><span className="readiness-icon is-ready"><KeyRound size={16} /></span><div><strong>Test credentials</strong><small>Build against simulated delivery first.</small></div><ArrowRight size={15} /></button>}<button onClick={() => go('/domains')}><span className={`readiness-icon ${verifiedDomains ? 'is-ready' : ''}`}><Globe2 size={16} /></span><div><strong>Sender domain</strong><small>{verifiedDomains ? `${verifiedDomains} verified domain${verifiedDomains === 1 ? '' : 's'}` : 'Verify DNS to unlock production.'}</small></div><ArrowRight size={15} /></button>{admin && <button onClick={() => go('/webhooks')}><span className={`readiness-icon ${activeWebhooks ? 'is-ready' : ''}`}><Webhook size={16} /></span><div><strong>Delivery webhooks</strong><small>{activeWebhooks ? `${activeWebhooks} active ${environment} endpoint${activeWebhooks === 1 ? '' : 's'}` : 'Connect final delivery events.'}</small></div><ArrowRight size={15} /></button>}</div>
      </Panel>
    </section>

    <Panel title="Recent email activity" eyebrow={environment === 'test' ? 'Simulated delivery' : 'Production delivery'} action={<button className="text-link" onClick={() => go('/emails')}>View all <ArrowRight size={13} /></button>}>
      <ErrorNotice error={emails.error} />
      {emails.loading && !emails.result ? <SkeletonRows rows={5} /> : recent.length ? <div className="activity-list">{recent.slice(0, 8).map(email => <button key={email.id} onClick={() => go(`/emails?message=${email.id}`)}><Badge value={email.status} /><div className="activity-list__main"><strong>{email.subject}</strong><span>{recipientSummary(email)}</span></div><time title={date(email.acceptedAt)}>{relativeDate(email.acceptedAt)}</time><ArrowRight size={14} /></button>)}</div> : <EmptyState title="No email activity yet" copy="Queue a test message and it will appear here automatically." />}
    </Panel>
  </div>
}

export function Emails({ environment }: { environment: Environment }) {
  const [searchParams, setSearchParams] = useSearchParams()
  const queryMessage = searchParams.get('message')
  const [offset, setOffset] = useState(0), [selected, setSelected] = useState<string | null>(queryMessage), [search, setSearch] = useState(''), [status, setStatus] = useState('all')
  const listPath = `/v1/emails?environment=${environment}&limit=25&offset=${offset}`
  const list = useResource<Email[]>(listPath, 4_000)
  const detail = useResource<Email>(selected ? `/v1/emails/${selected}` : null, selected ? emailDetailPoll : false)
  useEffect(() => { setOffset(0); setSearch(''); setStatus('all') }, [environment])
  useEffect(() => { setSelected(queryMessage) }, [queryMessage])
  function selectMessage(id: string | null) {
    setSelected(id)
    const next = new URLSearchParams(searchParams)
    if (id) next.set('message', id); else next.delete('message')
    setSearchParams(next, { replace: true })
  }
  const data = list.result?.data ?? []
  const filtered = useMemo(() => data.filter(email => {
    const haystack = `${email.subject} ${email.id} ${email.from} ${email.recipients.map(r => r.address).join(' ')}`.toLowerCase()
    return (!search || haystack.includes(search.toLowerCase())) && (status === 'all' || email.status === status)
  }), [data, search, status])
  const statuses = [...new Set(data.map(item => item.status))]
  const current = detail.result?.data

  return <div className="page-stack">
    <Panel title={`${environment === 'test' ? 'Test' : 'Production'} email activity`} eyebrow="Live message stream" action={<Refresh reload={list.reload} refreshing={list.refreshing} />}>
      <div className="toolbar"><label className="search-box"><Search size={15} /><input value={search} onChange={e => setSearch(e.target.value)} placeholder="Search subject, recipient, sender or ID" /></label><select aria-label="Filter by status" value={status} onChange={e => setStatus(e.target.value)}><option value="all">All statuses</option>{statuses.map(value => <option key={value} value={value}>{value.replaceAll('_', ' ')}</option>)}</select><span className="live-indicator"><i />Auto-refreshing</span></div>
      <ErrorNotice error={list.error} />
      {list.loading && !list.result ? <SkeletonRows rows={7} /> : <div className="table-wrap"><table className="data-table email-table"><thead><tr><th>Status</th><th>Message</th><th>Recipient</th><th>From</th><th>Accepted</th><th /></tr></thead><tbody>{filtered.map(email => <tr key={email.id} className={selected === email.id ? 'is-selected' : ''}><td><Badge value={email.status} /></td><td><button className="message-link" onClick={() => selectMessage(email.id)}><strong>{email.subject || '(No subject)'}</strong><code>{email.id}</code></button></td><td>{recipientSummary(email)}</td><td className="muted-cell">{email.from}</td><td><time title={date(email.acceptedAt)}>{relativeDate(email.acceptedAt)}</time></td><td><button className="icon-button" aria-label={`View ${email.subject}`} onClick={() => selectMessage(email.id)}><ArrowRight size={15} /></button></td></tr>)}</tbody></table></div>}
      {!list.loading && !filtered.length && !list.error && <EmptyState title={data.length ? 'No messages match these filters' : 'No messages in this environment yet'} copy={data.length ? 'Change the search or status filter.' : 'Send a message and this page will update automatically.'} />}
      <div className="pagination"><span>Showing page {Math.floor(offset / 25) + 1}</span><div><button className="button button--ghost button--compact" disabled={!offset || list.loading} onClick={() => setOffset(Math.max(0, offset - 25))}>Previous</button><button className="button button--ghost button--compact" disabled={!list.result?.hasMore || list.loading} onClick={() => setOffset(offset + 25)}>Next</button></div></div>
    </Panel>

    {selected && <DrawerSurface close={() => selectMessage(null)} labelledBy="email-detail-title"><header className="drawer__header"><div><p className="eyebrow">Message detail</p><h2 id="email-detail-title">{current?.subject || 'Loading message…'}</h2></div><button className="icon-button" aria-label="Close message detail" onClick={() => selectMessage(null)}><X size={18} /></button></header><div className="drawer__body"><ErrorNotice error={detail.error} />{detail.loading && !detail.result ? <SkeletonRows rows={6} /> : current && <>
      <div className="detail-status"><Badge value={current.status} /><EnvironmentPill environment={current.environment} />{!finalEmailStatus(current.status) && <span className="live-indicator"><i />Tracking</span>}</div>
      <ErrorNotice error={current.lastError} />
      <dl className="detail-grid"><div><dt>Message ID</dt><dd><code>{current.id}</code><CopyButton value={current.id} /></dd></div><div><dt>From</dt><dd>{current.from}</dd></div><div><dt>Accepted</dt><dd>{date(current.acceptedAt)}</dd></div><div><dt>Transport</dt><dd>{current.deliveryProvider || '—'}</dd></div>{current.providerMessageId && <div><dt>Provider ID</dt><dd><code>{current.providerMessageId}</code></dd></div>}</dl>
      <section className="detail-section"><h3>Recipients</h3><div className="recipient-list">{current.recipients.map(recipient => <div key={`${recipient.type}-${recipient.address}`}><span><b>{recipient.type.toUpperCase()}</b>{recipient.address}</span><Badge value={recipient.status} /></div>)}</div></section>
      <section className="detail-section"><h3>Delivery timeline</h3><div className="timeline"><div className="is-done"><i><Check size={12} /></i><div><strong>Accepted</strong><span>{date(current.acceptedAt)}</span></div></div>{current.sentAt && <div className="is-done"><i><Check size={12} /></i><div><strong>Sent to transport</strong><span>{date(current.sentAt)}</span></div></div>}{current.events?.slice().reverse().map((event, index) => <div key={event.id || `${event.type}-${index}`} className="is-done"><i><CircleDot size={10} /></i><div><strong>{event.type?.replace('email.', '').replaceAll('_', ' ') || 'Delivery event'}</strong><span>{date(event.occurredAt)}{event.recipient ? ` · ${event.recipient}` : ''}</span></div></div>)}{!finalEmailStatus(current.status) && <div className="is-live"><i><span /></i><div><strong>Waiting for final outcome</strong><span>This detail view refreshes automatically.</span></div></div>}</div></section>
      <section className="detail-section"><h3>Content</h3>{current.contentAvailable ? <div className="message-preview">{current.content?.html ? <iframe title="Email HTML preview" sandbox="" srcDoc={current.content.html} /> : <pre>{current.content?.text || '(No readable body)'}</pre>}</div> : <Notice tone="info">Message content expired or is temporarily unavailable. Delivery metadata remains visible.</Notice>}</section>
      {!!current.providerAttempts?.length && <section className="detail-section"><h3>Provider attempts</h3><div className="compact-list">{current.providerAttempts.map((attempt, index) => <div key={attempt.id || index}><span><strong>Attempt {attempt.attemptNumber ?? index + 1}</strong><small>{attempt.provider || 'transport'} · {attempt.status || 'unknown'}</small></span><time>{date(attempt.completedAt || attempt.startedAt)}</time></div>)}</div></section>}
      <details className="raw-details"><summary>Raw metadata and events <ChevronDown size={14} /></summary><h4>Metadata</h4><pre>{JSON.stringify(current.metadata, null, 2)}</pre><h4>Events</h4><pre>{JSON.stringify(current.events ?? [], null, 2)}</pre></details>
    </>}</div></DrawerSurface>}
  </div>
}

export function Domains({ admin, onProductionReady }: { admin: boolean; onProductionReady?: () => void }) {
  const cloudflareDomainKey = 'cs-mailer-cloudflare-domain'
  const list = useResource<Domain[]>('/v1/domains', 20_000), action = useAction()
  const [cloudflareCallbackDomain] = useState<string | null>(() => { try { const value = window.sessionStorage.getItem(cloudflareDomainKey); window.sessionStorage.removeItem(cloudflareDomainKey); return value } catch { return null } })
  const [selected, setSelected] = useState<string | null>(cloudflareCallbackDomain)
  const detail = useResource<Domain>(selected ? `/v1/domains/${selected}` : null, selected ? 10_000 : false)
  const pending = useRef<string[]>([]), checking = useRef(false)
  const [autoError, setAutoError] = useState(''), [autoChecking, setAutoChecking] = useState(false), [lastCheckedAt, setLastCheckedAt] = useState<Date | null>(null), [cloudflareConnecting, setCloudflareConnecting] = useState(false)
  const currentSearch = new URLSearchParams(window.location.search)
  const dnsResult = currentSearch.get('dns')
  pending.current = list.result?.data.filter(domain => domain.status === 'pending').map(domain => domain.id) ?? []
  useEffect(() => { if (!selected && cloudflareCallbackDomain) setSelected(cloudflareCallbackDomain) }, [cloudflareCallbackDomain, selected])
  useEffect(() => {
    if (!admin) return
    let cancelled = false
    const verifyPending = async () => {
      if (document.hidden || !navigator.onLine || checking.current || pending.current.length === 0) return
      checking.current = true; setAutoChecking(true); setAutoError('')
      try {
        let productionReady = false
        for (const id of pending.current) { const result = await api.post<Envelope<{ verified: boolean }>>(`/v1/domains/${id}/verify`, {}); productionReady ||= result.data.verified }
        if (productionReady) onProductionReady?.()
        if (!cancelled) { setLastCheckedAt(new Date()); await queryClient.invalidate(['/v1/domains']) }
      } catch (error) { if (!cancelled) setAutoError(error instanceof Error ? error.message : 'Automatic DNS verification will retry.') }
      finally { checking.current = false; if (!cancelled) setAutoChecking(false) }
    }
    const first = window.setTimeout(() => void verifyPending(), 1_500)
    const timer = window.setInterval(() => void verifyPending(), 15_000)
    const visible = () => { if (!document.hidden) void verifyPending() }
    document.addEventListener('visibilitychange', visible)
    return () => { cancelled = true; window.clearTimeout(first); window.clearInterval(timer); document.removeEventListener('visibilitychange', visible) }
  }, [admin, onProductionReady])
  const selectedDomain = detail.result?.data
  const requiredRecords = selectedDomain?.records.filter(record => record.required) ?? []
  const verifiedRequired = requiredRecords.filter(record => record.status === 'verified').length
  const callbackApplies = !cloudflareCallbackDomain || selected === cloudflareCallbackDomain
  const callbackSucceeded = dnsResult === 'published' && callbackApplies
  const checkLabel = autoChecking ? 'Checking DNS now…' : lastCheckedAt ? `Checked ${lastCheckedAt.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })}` : 'Automatic checks active'

  async function mutateAndRefresh(work: () => Promise<unknown>) { await work(); await queryClient.invalidate('/v1/domains') }

  return <div className="page-stack">
    <Panel title="Sending domains" eyebrow="Sender identity" action={<Refresh reload={list.reload} refreshing={list.refreshing} />}>
      <div className="domain-intro"><div><Globe2 size={20} /><div><strong>Verify a domain before production sending.</strong><p>CS Mailer generates the DNS records required by the configured transport and rechecks pending records automatically.</p></div></div>{pending.current.length > 0 && <span className="live-indicator"><i />{pending.current.length} pending</span>}</div>
      {admin && <form className="inline-create" onSubmit={e => { e.preventDefault(); const form = e.currentTarget, domain = String(new FormData(form).get('domain')); void action.run(async () => { const value = await api.post<Envelope<Domain>>('/v1/domains', { domain }); form.reset(); queryClient.update<Domain[]>('/v1/domains', current => current ? { ...current, data: [value.data, ...current.data.filter(item => item.id !== value.data.id)] } : { data: [value.data] }); queryClient.set(`/v1/domains/${value.data.id}`, { data: value.data }); setSelected(value.data.id); await queryClient.invalidate('/v1/domains') }) }}><Field label="Domain"><input name="domain" placeholder="mail.example.com" required /></Field><Submit busy={action.busy}>Add domain</Submit></form>}
      <ErrorNotice error={action.error || list.error || (!selected ? autoError : '')} />
      {list.loading && !list.result ? <SkeletonRows rows={4} /> : (list.result?.data.length ?? 0) > 0 ? <div className="domain-list">{list.result!.data.map(domain => { const required = domain.records.filter(record => record.required); const verified = required.filter(record => record.status === 'verified').length; return <article key={domain.id} className={selected === domain.id ? 'is-selected' : ''}><button className="domain-list__main" onClick={() => setSelected(domain.id)}><span className="domain-icon"><Globe2 size={17} /></span><span><strong>{domain.domain}</strong><small>{domain.provider} · {verified}/{required.length} required DNS records verified</small></span></button><Badge value={domain.status} /><div className="row-actions"><button className="icon-text-button" onClick={() => setSelected(domain.id)}>DNS records</button>{admin && domain.status === 'pending' && <button className="icon-text-button" disabled={action.busy || autoChecking} onClick={() => void action.run(async () => { const result = await api.post<Envelope<{ verified: boolean }>>(`/v1/domains/${domain.id}/verify`, {}); if (result.data.verified) onProductionReady?.(); setLastCheckedAt(new Date()); await queryClient.invalidate('/v1/domains') })}><RefreshCw className={autoChecking && pending.current.includes(domain.id) ? 'spin' : ''} size={13} />Verify</button>}{admin && <details className="action-menu"><summary aria-label="Domain actions"><MoreHorizontal size={16} /></summary><div>{domain.provider === 'stalwart' && <button disabled={action.busy || autoChecking} onClick={() => { if (confirm(`Rotate DKIM for ${domain.domain}? You must publish the new TXT record before sending resumes.`)) void action.run(async () => mutateAndRefresh(() => api.post(`/v1/domains/${domain.id}/rotate-dkim`, {}))) }}><RotateCw size={14} />Rotate DKIM</button>}<button className="danger-action" disabled={action.busy || autoChecking} onClick={() => { if (confirm(`Disable ${domain.domain}? Production sends from it will fail.`)) void action.run(async () => { const previous = list.result; queryClient.update<Domain[]>('/v1/domains', current => current ? { ...current, data: current.data.filter(item => item.id !== domain.id) } : current); if (selected === domain.id) setSelected(null); try { await api.delete(`/v1/domains/${domain.id}`) } catch (error) { if (previous) queryClient.set('/v1/domains', previous); throw error } queryClient.remove(`/v1/domains/${domain.id}`); await queryClient.invalidate('/v1/domains') }) }}><Trash2 size={14} />Disable domain</button></div></details>}</div></article>})}</div> : <EmptyState title="No sending domains" copy="Test sends can use sender@sandbox.mailer.invalid without DNS setup." />}
    </Panel>

    {selected && <DrawerSurface close={() => setSelected(null)} labelledBy="domain-detail-title" wide><header className="drawer__header"><div><p className="eyebrow">DNS configuration</p><h2 id="domain-detail-title">{selectedDomain?.domain || 'Loading domain…'}</h2></div><button className="icon-button" aria-label="Close DNS setup" onClick={() => setSelected(null)}><X size={18} /></button></header><div className="drawer__body"><ErrorNotice error={detail.error} />{detail.loading && !detail.result ? <SkeletonRows rows={6} /> : selectedDomain && <>
      <div className="detail-status"><Badge value={selectedDomain.status} /><span className="subtle-tag">Provider: {selectedDomain.provider}</span>{selectedDomain.status === 'pending' && <span className="live-indicator"><i />{checkLabel}</span>}</div>
      {callbackSucceeded && selectedDomain.status === 'verified' && <Notice tone="success">Cloudflare records were published and the domain is ready for production sending.</Notice>}
      {callbackApplies && dnsResult && dnsResult !== 'published' && <ErrorNotice error={dnsResult === 'cancelled' ? 'Cloudflare authorization was cancelled.' : dnsResult === 'expired' ? 'Cloudflare authorization expired. Start it again.' : 'Cloudflare could not add every record. Review the DNS table or try again.'} />}
      {selectedDomain.status === 'pending' && <div className="dns-progress"><div><span><Cloud size={17} /></span><div><strong>{callbackSucceeded ? 'Records published — waiting for DNS propagation' : 'Waiting for DNS records'}</strong><p>CS Mailer checks required records in the background every 15 seconds while this page is open.</p></div></div><div className="dns-progress__track"><span style={{ width: `${requiredRecords.length ? Math.round((verifiedRequired / requiredRecords.length) * 100) : 0}%` }} /></div><small>{verifiedRequired} of {requiredRecords.length} required records verified</small></div>}
      {admin && selectedDomain.dns_automation.includes('cloudflare') && selectedDomain.status === 'pending' && <div className="integration-callout"><span className="integration-callout__icon"><Cloud size={20} /></span><div><strong>Publish with Cloudflare</strong><p>Authorize temporary DNS access and CS Mailer can add the required records. The verification checks continue afterward.</p></div><button className="button button--secondary" disabled={action.busy || autoChecking || cloudflareConnecting} onClick={() => { setCloudflareConnecting(true); void action.run(async () => { try { window.sessionStorage.setItem(cloudflareDomainKey, selected) } catch { /* unavailable */ } const value = await api.post<Envelope<{ authorizationUrl: string }>>(`/v1/domains/${selected}/dns-automation/cloudflare`, {}); window.location.assign(value.data.authorizationUrl) }).finally(() => setCloudflareConnecting(false)) }}>{cloudflareConnecting ? <RefreshCw className="spin" size={14} /> : <ExternalLink size={14} />}{cloudflareConnecting ? 'Connecting…' : 'Connect Cloudflare'}</button></div>}
      <ErrorNotice error={action.error || autoError} />
      <section className="detail-section"><div className="section-heading"><div><h3>DNS records</h3><p>Copy the values exactly. Required records must verify before production sending is enabled.</p></div></div><div className="dns-records">{selectedDomain.records.map((record, index) => <article key={`${record.name}-${index}`}><div className="dns-records__top"><span className="dns-type">{['SPF', 'DMARC'].includes(record.record_type) ? `TXT · ${record.record_type}` : record.record_type}</span><Badge value={record.status} />{record.required ? <span className="required-tag">Required</span> : <span className="subtle-tag">Recommended</span>}</div><dl><div><dt>Name / Host</dt><dd><code>{record.name}</code><CopyButton value={record.name} /></dd></div><div><dt>Value</dt><dd><code>{record.value}</code><CopyButton value={record.value} /></dd></div></dl>{record.last_checked_at && <small>Last checked {relativeDate(record.last_checked_at)}</small>}</article>)}</div></section>
    </>}</div></DrawerSurface>}
  </div>
}

const scopes = ['emails:send', 'emails:read', 'domains:read', 'domains:write', 'webhooks:manage', 'suppressions:manage', 'workspace:read']
export function Keys({ productionEnabled }: { productionEnabled: boolean }) {
  const list = useResource<Key[]>('/v1/api-keys', 30_000), action = useAction(), [secret, setSecret] = useState(''), [createOpen, setCreateOpen] = useState(false)
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const form = event.currentTarget, data = new FormData(form)
    await action.run(async () => { const value = await api.post<Envelope<{ key: Key; secret: string }>>('/v1/api-keys', { name: data.get('name'), environment: data.get('environment'), scopes: data.getAll('scopes'), expires_in_days: Number(data.get('expiry')) }); queryClient.update<Key[]>('/v1/api-keys', current => current ? { ...current, data: [value.data.key, ...current.data.filter(item => item.id !== value.data.key.id)] } : { data: [value.data.key] }); setSecret(value.data.secret); form.reset(); setCreateOpen(false); await queryClient.invalidate('/v1/api-keys') })
  }
  return <div className="page-stack"><Panel title="API keys" eyebrow="Server-side credentials" action={<div className="panel-actions"><Refresh reload={list.reload} refreshing={list.refreshing} /><button className="button button--primary button--compact" onClick={() => setCreateOpen(value => !value)}><KeyRound size={14} />New key</button></div>}>
    {!productionEnabled && <Notice tone="info">Production keys unlock after at least one sending domain is verified. Test keys are available now.</Notice>}
    {createOpen && <form className="create-surface form-stack" onSubmit={create}><div className="form-two"><Field label="Key name"><input name="name" required maxLength={80} placeholder="Production API" /></Field><Field label="Environment"><select name="environment"><option value="test">Test · simulated delivery</option><option value="production" disabled={!productionEnabled}>Production · {productionEnabled ? 'real delivery' : 'verified domain required'}</option></select></Field></div><Field label="Expires after"><select name="expiry"><option value="90">90 days</option><option value="30">30 days</option><option value="365">365 days</option><option value="0">No expiry</option></select></Field><fieldset className="choice-grid"><legend>Permissions</legend>{scopes.map(scope => <label key={scope}><input type="checkbox" name="scopes" value={scope} defaultChecked={['emails:send', 'emails:read'].includes(scope)} /><span><code>{scope}</code></span></label>)}</fieldset><ErrorNotice error={action.error} /><div className="form-actions"><button type="button" className="button button--ghost" onClick={() => setCreateOpen(false)}>Cancel</button><Submit busy={action.busy}>Create API key</Submit></div></form>}
    <ErrorNotice error={list.error} />{list.loading && !list.result ? <SkeletonRows rows={5} /> : (list.result?.data.length ?? 0) ? <div className="table-wrap"><table className="data-table"><thead><tr><th>Name</th><th>Credential</th><th>Environment</th><th>Scopes</th><th>Last used</th><th>Expires</th><th /></tr></thead><tbody>{list.result!.data.map(key => <tr key={key.id}><td><strong>{key.name}</strong></td><td><code>{key.prefix}…</code></td><td><EnvironmentPill environment={key.environment} /></td><td><div className="chip-row">{key.scopes.map(scope => <span className="scope-chip" key={scope}>{scope}</span>)}</div></td><td>{relativeDate(key.lastUsedAt ?? key.last_used_at)}</td><td>{key.expiresAt || key.expires_at ? date(key.expiresAt ?? key.expires_at) : 'Never'}</td><td><details className="action-menu"><summary aria-label={`Actions for ${key.name}`}><MoreHorizontal size={16} /></summary><div><button disabled={action.busy} onClick={() => { if (confirm(`Rotate ${key.name}? The existing key stops working immediately.`)) void action.run(async () => { const value = await api.post<Envelope<{ secret: string }>>(`/v1/api-keys/${key.id}/rotate`, {}); setSecret(value.data.secret); await queryClient.invalidate('/v1/api-keys') }) }}><RotateCw size={14} />Rotate</button><button className="danger-action" disabled={action.busy} onClick={() => { if (confirm(`Revoke ${key.name}? This cannot be undone.`)) void action.run(async () => { const previous = list.result; queryClient.update<Key[]>('/v1/api-keys', current => current ? { ...current, data: current.data.filter(item => item.id !== key.id) } : current); try { await api.delete(`/v1/api-keys/${key.id}`) } catch (error) { if (previous) queryClient.set('/v1/api-keys', previous); throw error } await queryClient.invalidate('/v1/api-keys') }) }}><Trash2 size={14} />Revoke</button></div></details></td></tr>)}</tbody></table></div> : <EmptyState title="No active API keys" copy="Create a test key to start integrating CS Mailer from your server." />}
  </Panel>{secret && <Secret value={secret} close={() => setSecret('')} title="Copy your CS Mailer API key" />}</div>
}

const events = ['email.delivery', 'email.bounce', 'email.complaint', 'email.reject', 'email.rendering_failure', 'email.open', 'email.click']
export function Webhooks() {
  const list = useResource<Endpoint[]>('/v1/webhooks', 20_000), action = useAction(), [secret, setSecret] = useState(''), [selected, setSelected] = useState<string | null>(null), [attemptId, setAttemptId] = useState<string | null>(null), [createOpen, setCreateOpen] = useState(false)
  const deliveries = useResource<Delivery[]>(selected ? `/v1/webhooks/${selected}/deliveries` : null, selected ? 4_000 : false)
  const attempts = useResource<WebhookAttempt[]>(selected && attemptId ? `/v1/webhooks/${selected}/deliveries/${attemptId}/attempts` : null, selected && attemptId ? 10_000 : false)
  const selectedEndpoint = list.result?.data.find(endpoint => endpoint.id === selected)
  async function create(event: FormEvent<HTMLFormElement>) { event.preventDefault(); const form = event.currentTarget, data = new FormData(form); await action.run(async () => { const value = await api.post<Envelope<{ endpoint: Endpoint; secret: string }>>('/v1/webhooks', { url: data.get('url'), environment: data.get('environment'), subscriptions: data.getAll('events') }); queryClient.update<Endpoint[]>('/v1/webhooks', current => current ? { ...current, data: [value.data.endpoint, ...current.data.filter(item => item.id !== value.data.endpoint.id)] } : { data: [value.data.endpoint] }); setSelected(value.data.endpoint.id); setSecret(value.data.secret); form.reset(); setCreateOpen(false); await queryClient.invalidate('/v1/webhooks') }) }
  return <div className="page-stack"><Panel title="Webhook endpoints" eyebrow="Signed delivery events" action={<div className="panel-actions"><Refresh reload={list.reload} refreshing={list.refreshing} /><button className="button button--primary button--compact" onClick={() => setCreateOpen(value => !value)}><Webhook size={14} />Add endpoint</button></div>}>
    {createOpen && <form className="create-surface form-stack" onSubmit={create}><div className="form-two"><Field label="HTTPS endpoint"><input name="url" type="url" placeholder="https://hooks.example.com/cs-mailer" required /></Field><Field label="Environment"><select name="environment"><option value="test">Test</option><option value="production">Production</option></select></Field></div><fieldset className="choice-grid choice-grid--events"><legend>Subscribed events</legend>{events.map(event => <label key={event}><input name="events" type="checkbox" value={event} defaultChecked={['email.delivery', 'email.bounce', 'email.complaint'].includes(event)} /><span><code>{event}</code></span></label>)}</fieldset><p className="muted">Use a public HTTPS endpoint with trusted TLS. CS Mailer signs event requests with the endpoint secret.</p><ErrorNotice error={action.error} /><div className="form-actions"><button type="button" className="button button--ghost" onClick={() => setCreateOpen(false)}>Cancel</button><Submit busy={action.busy}>Create endpoint</Submit></div></form>}
    <ErrorNotice error={list.error} />{list.loading && !list.result ? <SkeletonRows rows={4} /> : (list.result?.data.length ?? 0) ? <div className="endpoint-list">{list.result!.data.map(endpoint => <article key={endpoint.id} className={selected === endpoint.id ? 'is-selected' : ''}><button className="endpoint-list__main" onClick={() => { setSelected(endpoint.id); setAttemptId(null) }}><span className={`endpoint-health ${endpoint.enabled ? 'is-on' : ''}`}><Webhook size={17} /></span><span><strong>{endpoint.url}</strong><small><EnvironmentPill environment={endpoint.environment} /> · {endpoint.subscriptions.length} subscribed events{endpoint.failureCount ? ` · ${endpoint.failureCount} failures` : ''}</small></span></button><div className="endpoint-list__status"><Badge value={endpoint.enabled ? 'enabled' : 'disabled'} />{endpoint.lastSuccessAt && <small>Success {relativeDate(endpoint.lastSuccessAt)}</small>}</div><div className="row-actions"><button className="icon-text-button" onClick={() => { setSelected(endpoint.id); setAttemptId(null) }}>Deliveries</button><button className="icon-text-button" disabled={action.busy} onClick={() => { const previous = endpoint.enabled; queryClient.update<Endpoint[]>('/v1/webhooks', current => current ? { ...current, data: current.data.map(item => item.id === endpoint.id ? { ...item, enabled: !previous } : item) } : current); void action.run(async () => { try { await api.patch(`/v1/webhooks/${endpoint.id}`, { enabled: !previous }) } catch (error) { await queryClient.invalidate('/v1/webhooks'); throw error } await queryClient.invalidate('/v1/webhooks') }) }}>{endpoint.enabled ? 'Disable' : 'Enable'}</button><details className="action-menu"><summary aria-label="Webhook actions"><MoreHorizontal size={16} /></summary><div><button disabled={action.busy} onClick={() => { if (confirm('Rotate the signing secret? Update your receiver immediately.')) void action.run(async () => { const value = await api.post<Envelope<{ secret: string }>>(`/v1/webhooks/${endpoint.id}/rotate-secret`, {}); setSecret(value.data.secret) }) }}><RotateCw size={14} />Rotate secret</button><button className="danger-action" disabled={action.busy} onClick={() => { if (confirm('Delete this endpoint and its delivery history?')) void action.run(async () => { const previous = list.result; queryClient.update<Endpoint[]>('/v1/webhooks', current => current ? { ...current, data: current.data.filter(item => item.id !== endpoint.id) } : current); if (selected === endpoint.id) { setSelected(null); setAttemptId(null) } try { await api.delete(`/v1/webhooks/${endpoint.id}`) } catch (error) { if (previous) queryClient.set('/v1/webhooks', previous); throw error } queryClient.remove(`/v1/webhooks/${endpoint.id}`); await queryClient.invalidate('/v1/webhooks') }) }}><Trash2 size={14} />Delete endpoint</button></div></details></div></article>)}</div> : <EmptyState title="No webhook endpoints" copy="Add an HTTPS endpoint to receive signed delivery events." />}
  </Panel>
  {selected && <Panel title="Webhook delivery monitor" eyebrow={selectedEndpoint?.url || 'Selected endpoint'} action={<div className="panel-actions"><span className="live-indicator"><i />Auto-refreshing</span><Refresh reload={deliveries.reload} refreshing={deliveries.refreshing} /></div>}><ErrorNotice error={deliveries.error || action.error} />{deliveries.loading && !deliveries.result ? <SkeletonRows rows={5} /> : (deliveries.result?.data.length ?? 0) ? <div className="delivery-list">{deliveries.result!.data.map(delivery => <article key={delivery.id} className={attemptId === delivery.id ? 'is-selected' : ''}><button onClick={() => setAttemptId(delivery.id)}><Badge value={delivery.status} /><span><strong>{delivery.eventType}</strong><small>{delivery.recipient || 'No recipient'} · {relativeDate(delivery.createdAt)}</small></span><span className="attempt-count">{delivery.attempts} attempt{delivery.attempts === 1 ? '' : 's'}</span><ArrowRight size={14} /></button>{delivery.lastError && <p className="inline-error"><AlertTriangle size={13} />{delivery.lastError}</p>}{delivery.status === 'failed' && <button className="retry-link" disabled={action.busy} onClick={() => void action.run(async () => { await api.post(`/v1/webhooks/${selected}/deliveries/${delivery.id}/retry`, {}); await queryClient.invalidate([`/v1/webhooks/${selected}/deliveries`, '/v1/webhooks']) })}><RefreshCw size={13} />Retry delivery</button>}</article>)}</div> : <EmptyState title="No webhook deliveries yet" copy="Send a test email with an endpoint subscribed to email.delivery." />}
    {attemptId && <section className="attempt-panel"><div className="section-heading"><div><h3>Delivery attempts</h3><p>HTTP response history for the selected event.</p></div><button className="icon-button" aria-label="Close attempt detail" onClick={() => setAttemptId(null)}><X size={15} /></button></div><ErrorNotice error={attempts.error} />{attempts.loading && !attempts.result ? <SkeletonRows rows={3} /> : <div className="attempt-table">{attempts.result?.data.map(attempt => <div key={attempt.attemptNumber}><span className={`http-code ${attempt.statusCode && attempt.statusCode >= 200 && attempt.statusCode < 300 ? 'is-good' : ''}`}>{attempt.statusCode ?? 'ERR'}</span><div><strong>Attempt {attempt.attemptNumber}</strong><small>{attempt.error || (attempt.deliveredAt ? `Delivered ${relativeDate(attempt.deliveredAt)}` : attempt.nextRetryAt ? `Retry ${relativeDate(attempt.nextRetryAt)}` : 'Completed')}</small></div><time>{date(attempt.createdAt)}</time></div>)}</div>}</section>}
  </Panel>}{secret && <Secret value={secret} close={() => setSecret('')} title="Copy your webhook signing secret" />}</div>
}

export function Suppressions() {
  const [offset, setOffset] = useState(0), [search, setSearch] = useState(''), listPath = `/v1/suppressions?limit=50&offset=${offset}`
  const list = useResource<Suppression[]>(listPath, 20_000), action = useAction()
  const data = list.result?.data ?? []
  const filtered = data.filter(item => !search || `${item.address} ${item.reason}`.toLowerCase().includes(search.toLowerCase()))
  async function create(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const form = event.currentTarget, address = String(new FormData(form).get('address') ?? '').trim().toLowerCase()
    const optimistic: Suppression = { id: `pending-${Date.now()}`, address, reason: 'manual', createdAt: new Date().toISOString() }
    if (offset === 0) queryClient.update<Suppression[]>(listPath, current => current ? { ...current, data: [optimistic, ...current.data] } : { data: [optimistic] })
    const ok = await action.run(async () => {
      await api.post('/v1/suppressions', { address })
      form.reset()
      if (offset !== 0) setOffset(0)
      await queryClient.invalidate('/v1/suppressions')
    })
    if (!ok) await queryClient.invalidate('/v1/suppressions')
  }
  return <div className="page-stack"><Panel title="Recipient suppressions" eyebrow="Reputation protection" action={<Refresh reload={list.reload} refreshing={list.refreshing} />}>
    <div className="domain-intro"><div><ShieldBan size={20} /><div><strong>Suppressed recipients are blocked across this workspace.</strong><p>Remove an address only after the original problem is resolved and permission to send is confirmed.</p></div></div></div>
    <form className="inline-create" onSubmit={create}><Field label="Email address"><input type="email" name="address" placeholder="recipient@example.com" required /></Field><Submit busy={action.busy}>Suppress recipient</Submit></form>
    <div className="toolbar toolbar--single"><label className="search-box"><Search size={15} /><input value={search} onChange={e => setSearch(e.target.value)} placeholder="Search suppressions on this page" /></label><span className="live-indicator"><i />Auto-refreshing</span></div>
    <ErrorNotice error={action.error || list.error} />{list.loading && !list.result ? <SkeletonRows rows={5} /> : filtered.length ? <div className="table-wrap"><table className="data-table"><thead><tr><th>Address</th><th>Reason</th><th>Added</th><th /></tr></thead><tbody>{filtered.map(item => <tr key={item.id} className={item.id.startsWith('pending-') ? 'is-pending-row' : ''}><td><strong>{item.address}</strong></td><td><span className="scope-chip">{item.reason}</span></td><td>{relativeDate(item.createdAt)}</td><td><button className="text-link text-link--danger" disabled={action.busy || item.id.startsWith('pending-')} onClick={() => { if (confirm(`Allow sending to ${item.address} again? Confirm validity and consent.`)) void action.run(async () => { queryClient.update<Suppression[]>(listPath, current => current ? { ...current, data: current.data.filter(value => value.id !== item.id) } : current); try { await api.delete(`/v1/suppressions/${item.id}`) } catch (error) { await queryClient.invalidate('/v1/suppressions'); throw error } await queryClient.invalidate('/v1/suppressions') }) }}>Remove</button></td></tr>)}</tbody></table></div> : <EmptyState title={data.length ? 'No suppressions match your search' : 'No recipient suppressions'} copy={data.length ? 'Try another address or reason.' : 'Addresses blocked by manual action or delivery feedback will appear here.'} />}
    <div className="pagination"><span>Showing page {Math.floor(offset / 50) + 1}</span><div><button className="button button--ghost button--compact" disabled={!offset} onClick={() => setOffset(Math.max(0, offset - 50))}>Previous</button><button className="button button--ghost button--compact" disabled={!list.result?.hasMore} onClick={() => setOffset(offset + 50)}>Next</button></div></div>
  </Panel></div>
}

export function DeveloperDocs() {
  const endpoint = `${window.location.origin}/api/v1/emails`
  const [tab, setTab] = useState<'curl' | 'node' | 'python'>('curl')
  const snippets = {
    curl: `curl ${endpoint} \\\n  -H "Authorization: Bearer $CS_MAILER_KEY" \\\n  -H "Idempotency-Key: user-42-welcome-v1" \\\n  -H "Content-Type: application/json" \\\n  -d '{"from":"sender@sandbox.mailer.invalid","to":["you@example.com"],"subject":"Welcome","text":"Your account is ready."}'`,
    node: `const response = await fetch("${endpoint}", {\n  method: "POST",\n  headers: {\n    Authorization: \`Bearer \${process.env.CS_MAILER_KEY}\`,\n    "Content-Type": "application/json",\n    "Idempotency-Key": \`user-\${user.id}-welcome-v1\`\n  },\n  body: JSON.stringify({\n    from: "Acme <no-reply@mail.example.com>",\n    to: [user.email],\n    subject: "Welcome to Acme",\n    text: "Your account is ready."\n  })\n});\nif (!response.ok) throw new Error(\`CS Mailer rejected request: \${response.status}\`);\nconst { data } = await response.json();`,
    python: `response = requests.post(\n    "${endpoint}",\n    headers={\n        "Authorization": f"Bearer {os.environ['CS_MAILER_KEY']}",\n        "Idempotency-Key": f"user-{user_id}-welcome-v1",\n    },\n    json={\n        "from": "Acme <no-reply@mail.example.com>",\n        "to": [recipient],\n        "subject": "Welcome to Acme",\n        "text": "Your account is ready.",\n    },\n    timeout=10,\n)\nresponse.raise_for_status()`
  }
  return <div className="docs-layout"><aside className="docs-index"><p className="eyebrow">API guide</p><a href="#quickstart">Quick start</a><a href="#lifecycle">Delivery lifecycle</a><a href="#retries">Reliable retries</a><a href="#webhook-signatures">Webhook signatures</a><a href="#scope">Current API scope</a></aside><div className="docs-content"><section className="docs-hero" id="quickstart"><span><Code2 size={21} /></span><p className="eyebrow">CS Mailer API</p><h2>Send your first transactional email.</h2><p>Create a test API key with <code>emails:send</code> and <code>emails:read</code>. Keep credentials in trusted server-side storage and use a stable idempotency key for each application event.</p></section><section className="code-example"><div className="code-tabs">{(['curl', 'node', 'python'] as const).map(value => <button key={value} className={tab === value ? 'is-active' : ''} onClick={() => setTab(value)}>{value === 'node' ? 'Node.js' : value === 'python' ? 'Python' : 'cURL'}</button>)}<CopyButton value={snippets[tab]} label="Copy" /></div><pre><code>{snippets[tab]}</code></pre></section><section className="docs-section" id="lifecycle"><p className="docs-number">01</p><div><h3>Understand the delivery lifecycle</h3><p>An accepted send request returns HTTP <code>202</code> and starts in <code>queued</code>. A <code>sent</code> state means the configured delivery transport accepted the message. A <code>delivered</code> event means the recipient server accepted it.</p><div className="docs-flow"><span>202 accepted</span><ArrowRight size={14} /><span>queued</span><ArrowRight size={14} /><span>sent</span><ArrowRight size={14} /><span>delivered / failed</span></div></div></section><section className="docs-section" id="retries"><p className="docs-number">02</p><div><h3>Retry safely</h3><p>Use a stable, event-specific idempotency key for verification, password reset, receipts, alerts, and other transactional mail. Retry network errors and HTTP 5xx or 429 responses with the same key. Create a new key only when you intend to create another email.</p></div></section><section className="docs-section" id="webhook-signatures"><p className="docs-number">03</p><div><h3>Verify webhook requests before parsing</h3><p>Compute HMAC-SHA256 over <code>webhook-id + "." + webhook-timestamp + "." + rawBody</code> using the complete <code>whsec_</code> secret. Compare the unpadded base64url digest with the <code>v1,</code> signature using constant-time comparison, reject stale timestamps, and deduplicate <code>webhook-id</code>.</p><Notice tone="info">Webhook delivery is at least once. Receivers should be idempotent and return a 2xx response promptly.</Notice></div></section><section className="docs-section" id="scope"><p className="docs-number">04</p><div><h3>Current API scope</h3><p>CS Mailer currently supports up to 50 total To/CC/BCC recipients, text or HTML bodies, reply-to, metadata, and attachments. Test delivery is simulated. Production delivery requires a verified sending domain.</p><div className="chip-row"><span className="scope-chip">emails:send</span><span className="scope-chip">emails:read</span><span className="scope-chip">domains:read</span><span className="scope-chip">domains:write</span><span className="scope-chip">webhooks:manage</span><span className="scope-chip">suppressions:manage</span><span className="scope-chip">workspace:read</span></div></div></section></div></div>
}
