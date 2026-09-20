import { useCallback, useEffect, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import {
  Activity, BookOpen, ChevronRight, Globe2, KeyRound, LogOut, Mail, Menu,
  Send, ShieldBan, UserRound, Webhook, X
} from 'lucide-react'
import { api, ApiError } from '../../lib/api/client'
import { BrandLogo, Envelope, Environment, ErrorNotice, queryClient, Session, errorText, useOnlineStatus } from './shared'
import { Authentication } from './Authentication'
import { LandingPage } from './LandingPage'
import { DeveloperDocs, Domains, Emails, Keys, Overview, Suppressions, Webhooks } from './Pages'
import { LegalPage, NotFoundPage } from './PublicPages'
import { SendDialog } from './SendDialog'
import './console.css'

type NavItem = { path: string; label: string; description: string; icon: typeof Activity; admin?: boolean; group: 'Operate' | 'Configure' | 'Develop' }
const navigation: NavItem[] = [
  { path: '/overview', label: 'Overview', description: 'Live workspace and delivery health.', icon: Activity, group: 'Operate' },
  { path: '/emails', label: 'Emails', description: 'Track every accepted message and delivery outcome.', icon: Mail, group: 'Operate' },
  { path: '/domains', label: 'Domains', description: 'Configure sender identity and DNS readiness.', icon: Globe2, group: 'Configure' },
  { path: '/api-keys', label: 'API keys', description: 'Issue scoped credentials for server-side integrations.', icon: KeyRound, admin: true, group: 'Configure' },
  { path: '/webhooks', label: 'Webhooks', description: 'Receive signed delivery events and inspect attempts.', icon: Webhook, admin: true, group: 'Configure' },
  { path: '/suppressions', label: 'Suppressions', description: 'Protect reputation by blocking invalid recipients.', icon: ShieldBan, admin: true, group: 'Configure' },
  { path: '/developers', label: 'API guide', description: 'Integrate CS Mailer safely and predictably.', icon: BookOpen, group: 'Develop' }
]
const environmentKey = 'cs-mailer-environment'
const authPaths = ['/login', '/signup', '/forgot-password', '/reset-password', '/verify-email', '/resend-verification']
const publicPaths = ['/', '/terms', '/privacy']

function savedEnvironment(): Environment {
  try { return window.localStorage.getItem(environmentKey) === 'production' ? 'production' : 'test' } catch { return 'test' }
}

export default function Console() {
  const [session, setSession] = useState<Session | null>(null), [loading, setLoading] = useState(true), [sessionError, setSessionError] = useState('')
  const [environment, setEnvironment] = useState<Environment>(savedEnvironment), [sendOpen, setSendOpen] = useState(false), [mobileOpen, setMobileOpen] = useState(false), [profileOpen, setProfileOpen] = useState(false)
  const navigate = useNavigate(), location = useLocation(), online = useOnlineStatus()
  const sessionRef = useRef<Session | null>(null)
  useEffect(() => { sessionRef.current = session }, [session])
  const loadSession = useCallback(async (background = false) => {
    try { const value = await api.get<Envelope<Session>>('/v1/auth/session'); setSession(value.data); setSessionError('') }
    catch (error) {
      if (error instanceof ApiError && error.status === 401) { queryClient.remove('/v1/'); setSession(null) }
      else if (!background || !sessionRef.current) setSessionError(errorText(error))
    } finally { if (!background) setLoading(false) }
  }, [])
  const refreshProductionAccess = useCallback(() => { void loadSession(true) }, [loadSession])

  useEffect(() => {
    void loadSession(false)
    const expired = () => { setSession(null); setSendOpen(false); queryClient.remove('/v1/') }
    window.addEventListener('mailer:session-expired', expired)
    return () => window.removeEventListener('mailer:session-expired', expired)
  }, [loadSession])
  useEffect(() => {
    if (!session) return
    const refresh = () => { if (!document.hidden && navigator.onLine) void loadSession(true) }
    const timer = window.setInterval(refresh, 15_000)
    window.addEventListener('focus', refresh); window.addEventListener('online', refresh)
    document.addEventListener('visibilitychange', refresh)
    return () => { window.clearInterval(timer); window.removeEventListener('focus', refresh); window.removeEventListener('online', refresh); document.removeEventListener('visibilitychange', refresh) }
  }, [session?.user.id, loadSession])
  useEffect(() => {
    if (!session) return
    if (environment === 'production' && !session.workspace.production_enabled) { setEnvironment('test'); return }
    try { window.localStorage.setItem(environmentKey, environment) } catch { /* private browsing */ }
  }, [environment, session])
  useEffect(() => { setMobileOpen(false); setProfileOpen(false); window.scrollTo({ top: 0 }) }, [location.pathname])
  useEffect(() => {
    if (!mobileOpen) return
    const previousOverflow = document.body.style.overflow
    const closeMenu = (event: KeyboardEvent) => { if (event.key === 'Escape') setMobileOpen(false) }
    document.body.style.overflow = 'hidden'
    document.addEventListener('keydown', closeMenu)
    return () => { document.body.style.overflow = previousOverflow; document.removeEventListener('keydown', closeMenu) }
  }, [mobileOpen])
  useEffect(() => {
    const names: Record<string, string> = { '/': 'Developer Email Infrastructure', '/terms': 'Terms of Service', '/privacy': 'Privacy Policy', '/login': 'Sign in', '/signup': 'Create account', '/forgot-password': 'Reset password', '/reset-password': 'Choose a new password', '/verify-email': 'Verify email', '/resend-verification': 'Resend verification' }
    const consoleName = navigation.find(item => item.path === location.pathname)?.label
    document.title = `CS Mailer · ${names[location.pathname] ?? consoleName ?? 'Developer Email Infrastructure'}`
  }, [location.pathname])

  if (loading) return <main className="boot-screen"><div className="boot-mark"><img src="/cs-mailer-logo.png" alt="" /></div><BrandLogo /><p><span />Connecting to your workspace</p></main>
  if (location.pathname === '/') return <LandingPage signedIn={Boolean(session)} signIn={() => navigate('/login')} createAccount={() => navigate(session ? '/overview' : '/signup')} />
  if (location.pathname === '/terms') return <LegalPage kind="terms" />
  if (location.pathname === '/privacy') return <LegalPage kind="privacy" />
  if (sessionError && !session && !authPaths.includes(location.pathname)) return <main className="standalone-error"><BrandLogo /><ErrorNotice error={sessionError} /><button className="button button--primary" onClick={() => void loadSession(false)}>Try again</button></main>
  if (!session || authPaths.includes(location.pathname)) {
    if (!authPaths.includes(location.pathname) && !publicPaths.includes(location.pathname)) return <NotFoundPage signedIn={false} />
    return <Authentication signedIn={value => { setSession(value); setSessionError('') }} />
  }

  const admin = ['owner', 'admin'].includes(session.user.role)
  const route = navigation.find(item => item.path === location.pathname && (!item.admin || admin))
  if (!route) return <NotFoundPage signedIn />
  const visibleNavigation = navigation.filter(item => !item.admin || admin)
  const groupedNavigation = ['Operate', 'Configure', 'Develop'].map(group => ({ group, items: visibleNavigation.filter(item => item.group === group) })).filter(item => item.items.length)
  function go(path: string) { navigate(path); setMobileOpen(false) }
  function changeEnvironment(next: Environment) {
    if (next === 'production' && !session!.workspace.production_enabled) return
    setEnvironment(next); setSendOpen(false)
  }
  async function signOut() {
    try { await api.post('/v1/auth/logout', {}) } finally { queryClient.remove('/v1/'); setSession(null); navigate('/login', { replace: true }) }
  }

  const page = route.path === '/emails' ? <Emails environment={environment} />
    : route.path === '/domains' ? <Domains admin={admin} onProductionReady={refreshProductionAccess} />
      : route.path === '/api-keys' ? <Keys productionEnabled={session.workspace.production_enabled} />
        : route.path === '/webhooks' ? <Webhooks />
          : route.path === '/suppressions' ? <Suppressions />
            : route.path === '/developers' ? <DeveloperDocs />
              : <Overview environment={environment} session={session} admin={admin} send={() => setSendOpen(true)} go={go} />

  return <div className="app-shell">
    <aside className={`app-sidebar ${mobileOpen ? 'is-open' : ''}`}>
      <div className="sidebar-brand"><button className="brand-button" onClick={() => go('/overview')}><BrandLogo /></button><button className="icon-button sidebar-close" aria-label="Close navigation" onClick={() => setMobileOpen(false)}><X size={17} /></button></div>
      <div className="workspace-card"><span className="workspace-avatar">{session.workspace.name.slice(0, 1).toUpperCase()}</span><div><small>Workspace</small><strong>{session.workspace.name}</strong></div><ChevronRight size={15} /></div>
      <nav className="sidebar-nav" aria-label="Primary navigation">{groupedNavigation.map(group => <div className="nav-group" key={group.group}><span>{group.group}</span>{group.items.map(item => <button className={route.path === item.path ? 'is-active' : ''} key={item.path} onClick={() => go(item.path)}><item.icon size={16} /><b>{item.label}</b>{route.path === item.path && <i />}</button>)}</div>)}</nav>
      <div className={`sidebar-live ${online ? '' : 'is-offline'}`.trim()}><span><i />{online ? 'Console sync active' : 'Offline'}</span><small>{online ? 'Views refresh in the background.' : 'Showing cached data until connection returns.'}</small></div>
      <div className="sidebar-account"><button onClick={() => setProfileOpen(value => !value)} aria-expanded={profileOpen}><span className="user-avatar"><UserRound size={15} /></span><span><strong>{session.user.name}</strong><small>{session.user.email}</small></span><ChevronRight className={profileOpen ? 'is-open' : ''} size={15} /></button>{profileOpen && <div className="account-popover"><div><small>Role</small><strong>{session.user.role}</strong></div><button onClick={() => void signOut()}><LogOut size={14} />Sign out</button></div>}</div>
    </aside>

    <main className="app-main">
      <header className="app-topbar"><button className="icon-button mobile-menu" aria-label="Open navigation" onClick={() => setMobileOpen(true)}><Menu size={18} /></button><div className="topbar-breadcrumb"><span>CS Mailer</span><ChevronRight size={13} /><strong>{route.label}</strong></div><div className="topbar-actions"><span className={`connection-pill ${online ? 'is-online' : 'is-offline'}`} role="status"><i />{online ? 'Live' : 'Offline'}</span><div className="environment-switch" aria-label="Environment selector"><button className={environment === 'test' ? 'is-active' : ''} onClick={() => changeEnvironment('test')}><i />Test</button><button className={environment === 'production' ? 'is-active' : ''} disabled={!session.workspace.production_enabled} title={!session.workspace.production_enabled ? 'Verify a sender domain to unlock production' : undefined} onClick={() => changeEnvironment('production')}><i />Production</button></div></div></header>
      <div className="app-page">{!online && <div className="connectivity-warning" role="status"><strong>Connection lost.</strong><span>CS Mailer is keeping the current view available and will refresh changed data automatically when you reconnect.</span></div>}<section className="page-heading"><div><div className="page-heading__eyebrow"><span>{session.workspace.name}</span><EnvironmentPillInternal environment={environment} /></div><h1>{route.label}</h1><p>{route.description}</p></div>{admin && route.path !== '/overview' && <button className="button button--primary" disabled={environment === 'production' && session.workspace.sending_paused} onClick={() => setSendOpen(true)}><Send size={15} />Send email</button>}</section>
        <ErrorNotice error={sessionError} />{session.workspace.sending_paused && <div className="global-warning"><ShieldBan size={17} /><div><strong>Production sending is paused</strong><p>{session.workspace.sending_pause_reason || 'Rotate affected API keys and contact the operator before resuming.'}</p></div></div>}
        <div className="route-content" key={route.path}>{page}</div>
        <footer className="app-footer"><span>CS Mailer · Developer email infrastructure</span><div><button onClick={() => navigate('/privacy')}>Privacy</button><button onClick={() => navigate('/terms')}>Terms</button></div></footer>
      </div>
    </main>
    {mobileOpen && <button className="mobile-backdrop" aria-label="Close navigation" onClick={() => setMobileOpen(false)} />}
    {sendOpen && <SendDialog environment={environment} close={() => setSendOpen(false)} queued={() => { void loadSession(true) }} />}
  </div>
}

function EnvironmentPillInternal({ environment }: { environment: Environment }) { return <span className={`environment-label environment-label--${environment}`}>{environment}</span> }
