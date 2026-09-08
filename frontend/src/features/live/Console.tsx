import { useCallback, useEffect, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { Activity, Globe2, KeyRound, LogOut, Mail, Menu, Send, ShieldBan, Webhook, X, BookOpen } from 'lucide-react'
import { api, ApiError } from '../../lib/api/client'
import { Envelope, Environment, Session, ErrorNotice, Panel, Refresh, errorText, useAction } from './shared'
import { Authentication } from './Authentication'
import { LandingPage } from './LandingPage'
import { Domains, Emails, Keys, Suppressions, Webhooks } from './Pages'
import { SendDialog } from './SendDialog'
import './console.css'

function DeveloperDocs() {
  const endpoint = `${window.location.origin}/api/v1/emails`
  return <Panel title="Integrate with the API">
    <p>Create a test key with <code>emails:send</code> and <code>emails:read</code>. Keep it in a server-side secret store; never put it in browser or mobile code. Production delivery uses the platform's independent SMTP infrastructure.</p>
    <h3>cURL</h3>
    <pre>{`curl ${endpoint} \\\n+  -H "Authorization: Bearer $MAILER_API_KEY" \\\n+  -H "Idempotency-Key: user-42-welcome-v1" \\\n+  -H "Content-Type: application/json" \\\n+  -d '{"from":"sender@sandbox.mailer.invalid","to":["you@example.com"],"subject":"Welcome","text":"Your account is ready."}'`}</pre>
    <h3>Node.js</h3>
    <pre>{`const response = await fetch("${endpoint}", {
  method: "POST",
  headers: {
    Authorization: \`Bearer \${process.env.MAILER_API_KEY}\`,
    "Content-Type": "application/json",
    "Idempotency-Key": \`user-\${user.id}-welcome-v1\`
  },
  body: JSON.stringify({
    from: "Acme <no-reply@mail.example.com>",
    to: [user.email],
    subject: "Welcome to Acme",
    text: "Your account is ready."
  })
});
if (!response.ok) throw new Error(\`Mailer rejected request: \${response.status}\`);
const { data } = await response.json(); // data.id and initial status "queued"`}</pre>
    <h3>Python</h3>
    <pre>{`response = requests.post(
    "${endpoint}",
    headers={
        "Authorization": f"Bearer {os.environ['MAILER_API_KEY']}",
        "Idempotency-Key": f"user-{user_id}-welcome-v1",
    },
    json={
        "from": "Acme <no-reply@mail.example.com>",
        "to": [recipient],
        "subject": "Welcome to Acme",
        "text": "Your account is ready.",
    },
    timeout=10,
)
response.raise_for_status()`}</pre>
    <h3>Reliable application email</h3>
    <p>Use a stable, event-specific idempotency key for verification, password reset, receipts, alerts, and other transactional mail. Retry network errors and HTTP 5xx/429 responses with the same key. Do not create a new key unless you intend to create another email.</p>
    <p>An accepted request returns HTTP 202 and status <code>queued</code>. <code>sent</code> means the delivery provider accepted the message; only <code>delivered</code> means the recipient server accepted it. Read status using <code>GET /v1/emails/{'{id}'}</code>, or consume signed webhooks for final outcomes.</p>
    <p>A production key requires a verified sender domain. Test delivery is simulated and defaults to delivered. Use <code>bounce@simulator.mailer.invalid</code> or <code>complaint@simulator.mailer.invalid</code> to exercise feedback and suppression.</p>
    <h3>Webhook verification</h3>
    <p>Verify raw request bytes before parsing JSON. Sign <code>webhook-id + "." + webhook-timestamp + "." + rawBody</code> using HMAC-SHA256, with the complete <code>whsec_</code> secret as the UTF-8 key. Compare the unpadded base64url digest to the value after <code>v1,</code> in <code>webhook-signature</code> using constant-time comparison. Reject timestamps more than five minutes away and deduplicate <code>webhook-id</code>.</p>
    <p>Payloads use names such as <code>email.delivery</code>. <code>data.emailId</code> matches the send response, and <code>data.environment</code> separates test and production. Delivery is at least once, so receivers must be idempotent and return 2xx promptly.</p>
    <h3>Current API scope</h3>
    <p>Up to 50 total To/CC/BCC recipients, text/HTML, <code>reply_to</code>, metadata, and attachments. Custom headers/tags, templates, billing, MFA, and team management are not currently available. The monthly limit counts accepted message submissions, including tests.</p>
  </Panel>
}
const navigation = [{ path: '/overview', label: 'Overview', icon: Activity }, { path: '/emails', label: 'Emails', icon: Mail }, { path: '/domains', label: 'Domains', icon: Globe2 }, { path: '/api-keys', label: 'API keys', icon: KeyRound, admin: true }, { path: '/webhooks', label: 'Webhooks', icon: Webhook, admin: true }, { path: '/suppressions', label: 'Suppressions', icon: ShieldBan, admin: true }, { path: '/developers', label: 'API guide', icon: BookOpen }]
const environmentKey = 'crescentsphere-mailer-environment'
const authPaths = ['/login', '/signup', '/forgot-password', '/reset-password', '/verify-email', '/resend-verification']

function savedEnvironment(): Environment {
  try { return window.localStorage.getItem(environmentKey) === 'production' ? 'production' : 'test' } catch { return 'test' }
}

export default function Console() {
  const [session, setSession] = useState<Session | null>(null), [loading, setLoading] = useState(true), [sessionError, setSessionError] = useState('')
  const [environment, setEnvironment] = useState<Environment>(savedEnvironment), [sendOpen, setSendOpen] = useState(false), [mobileOpen, setMobileOpen] = useState(false), [revision, setRevision] = useState(0)
  const navigate = useNavigate(), location = useLocation(), action = useAction()
  const loadSession = useCallback(async () => {
    try { const value = await api.get<Envelope<Session>>('/v1/auth/session'); setSession(value.data); setSessionError('') } catch (error) {
      if (error instanceof ApiError && error.status === 401) setSession(null); else setSessionError(errorText(error))
    } finally { setLoading(false) }
  }, [])
  const refreshProductionAccess = useCallback(() => { void loadSession() }, [loadSession])
  useEffect(() => { void loadSession(); const expired = () => { setSession(null); setSendOpen(false) }; window.addEventListener('mailer:session-expired', expired); return () => window.removeEventListener('mailer:session-expired', expired) }, [loadSession])
  useEffect(() => {
    if (!session) return
    if (environment === 'production' && !session.workspace.production_enabled) { setEnvironment('test'); return }
    try { window.localStorage.setItem(environmentKey, environment) } catch { /* Storage can be unavailable in private browsing. */ }
  }, [environment, session])
  if (loading) return <main className="live-auth" role="status">Loading your workspace…</main>
  if (location.pathname === '/') return <LandingPage signedIn={Boolean(session)} signIn={() => navigate('/login')} createAccount={() => navigate(session ? '/overview' : '/signup')} />
  if (sessionError && !session) return <main className="live-auth"><ErrorNotice error={sessionError} /><Refresh reload={() => void loadSession()} /></main>
  if (!session || authPaths.includes(location.pathname)) return <Authentication signedIn={setSession} />
  const admin = ['owner', 'admin'].includes(session.user.role)
  const route = navigation.find(item => item.path === location.pathname && (!item.admin || admin)) ?? navigation[0]
  function go(path: string) { navigate(path); setMobileOpen(false) }
  const page = route.path === '/emails' ? <Emails environment={environment} /> : route.path === '/domains' ? <Domains admin={admin} onProductionReady={refreshProductionAccess} /> : route.path === '/api-keys' ? <Keys productionEnabled={session.workspace.production_enabled} /> : route.path === '/webhooks' ? <Webhooks /> : route.path === '/suppressions' ? <Suppressions /> : route.path === '/developers' ? <DeveloperDocs /> : <><div className="metric-grid live-metrics"><Panel title="Monthly submissions"><strong className="metric-card__value">{session.workspace.usage.sent.toLocaleString()}</strong><p>of {session.workspace.usage.limit.toLocaleString()} accepted-message limit, including tests</p></Panel><Panel title="Current environment"><strong className="metric-card__value">{environment}</strong><p>{environment === 'test' ? 'Simulated delivery. No recipient email.' : 'Real email through the configured production transport.'}</p></Panel></div><Panel title="Get your integration running"><ol className="live-steps"><li>Create a test key and validate your integration safely.</li><li>Add and verify a sending domain to unlock production.</li><li>Create a production key and send real email through your production transport.</li></ol><button className="button button--secondary" onClick={() => go('/developers')}>Read the API guide</button></Panel><Emails environment={environment} /></>
  return <div className="app-shell"><aside className={`sidebar ${mobileOpen ? 'is-open' : ''}`}><div className="sidebar__top"><button className="brand" onClick={() => go('/overview')}><span className="brand__mark"><img src="/crescentsphere-mark.svg" alt="" /></span><span className="brand__name">Crescent<span>mail</span></span></button><button className="icon-button sidebar__close" aria-label="Close navigation" onClick={() => setMobileOpen(false)}><X size={16} /></button></div><div className="workspace-switcher"><div className="workspace-switcher__copy"><span className="eyebrow">Workspace</span><strong>{session.workspace.name}</strong></div></div><nav className="nav" aria-label="Primary navigation">{navigation.filter(item => !item.admin || admin).map(item => <button className={`nav__item ${route.path === item.path ? 'nav__item--active' : ''}`} key={item.path} onClick={() => go(item.path)}><item.icon size={16} />{item.label}</button>)}</nav><div className="sidebar__bottom"><p><strong>{session.user.name}</strong><br /><small>{session.user.email} · {session.user.role}</small></p><ErrorNotice error={action.error} /><button className="nav__item" disabled={action.busy} onClick={() => action.run(async () => { await api.post('/v1/auth/logout', {}); setSession(null); navigate('/login') })}><LogOut size={16} />Sign out</button></div></aside><main className="main-content"><header className="topbar"><button className="icon-button menu-trigger" aria-label="Open navigation" onClick={() => setMobileOpen(true)}><Menu size={18} /></button><strong>{route.label}</strong><div className="topbar__actions"><label className="live-environment">Environment<select aria-label="Environment" value={environment} onChange={e => { setEnvironment(e.target.value as Environment); setSendOpen(false) }}><option value="test">Test</option><option value="production" disabled={!session.workspace.production_enabled}>Production{session.workspace.production_enabled ? '' : ' — verified domain required'}</option></select></label></div></header><div className="page-wrap react-page"><section className="page-heading"><div><p className="eyebrow">{session.workspace.name}</p><h1>{route.label}</h1></div>{admin && <button className="button button--primary" disabled={environment === 'production' && session.workspace.sending_paused} onClick={() => setSendOpen(true)}><Send size={15} />Send email</button>}</section><ErrorNotice error={sessionError} />{session.workspace.sending_paused && <p className="live-notice">Production sending is paused for this workspace. Rotate affected API keys and contact the operator before resuming.</p>}<div key={`${route.path}:${revision}`}>{page}</div><footer className="page-footer">CrescentSphere Mailer · Developer email infrastructure</footer></div></main>{mobileOpen && <button className="mobile-sidebar-backdrop" aria-label="Close navigation" onClick={() => setMobileOpen(false)} />}{sendOpen && <SendDialog environment={environment} close={() => setSendOpen(false)} queued={() => { setRevision(value => value + 1); void loadSession() }} />}</div>
}
