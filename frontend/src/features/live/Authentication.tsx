import { FormEvent, useEffect, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { Mail } from 'lucide-react'
import { api } from '../../lib/api/client'
import { Envelope, Session, useAction, useResource, Panel, Field, ErrorNotice, Submit } from './shared'

type AuthConfig = { passwordRecovery: boolean; turnstileSiteKey?: string }
type TurnstileApi = { render: (element: HTMLElement, options: Record<string, unknown>) => string; remove: (id: string) => void }
declare global { interface Window { turnstile?: TurnstileApi } }

function Turnstile({ siteKey, token }: { siteKey?: string; token: (value: string) => void }) {
  const container = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!siteKey || !container.current) return
    let widget = '', cancelled = false
    const render = () => {
      if (cancelled || widget || !container.current || !window.turnstile) return
      widget = window.turnstile.render(container.current, { sitekey: siteKey, action: 'signup', theme: 'auto', callback: (value: string) => token(value), 'expired-callback': () => token(''), 'error-callback': () => token('') })
    }
    let script = document.querySelector<HTMLScriptElement>('script[data-mailer-turnstile]')
    if (!script) { script = document.createElement('script'); script.dataset.mailerTurnstile = 'true'; script.src = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit'; script.async = true; script.defer = true; document.head.appendChild(script) }
    script.addEventListener('load', render); render()
    return () => { cancelled = true; script?.removeEventListener('load', render); if (widget) window.turnstile?.remove(widget) }
  }, [siteKey, token])
  return siteKey ? <div ref={container} aria-label="Security check" /> : null
}

export function Authentication({ signedIn }: { signedIn: (session: Session) => void }) {
  const location = useLocation(), navigate = useNavigate(), action = useAction()
  const mode = location.pathname === '/signup' ? 'signup' : location.pathname === '/forgot-password' ? 'forgot' : location.pathname === '/reset-password' ? 'reset' : location.pathname === '/verify-email' ? 'verify' : location.pathname === '/resend-verification' ? 'resend' : 'login'
  const config = useResource<AuthConfig>('/v1/auth/config')
  const [notice, setNotice] = useState(''), [turnstileToken, setTurnstileToken] = useState('')
  useEffect(() => { setNotice(''); setTurnstileToken('') }, [mode])
  useEffect(() => {
    if (mode !== 'verify') return
    const token = new URLSearchParams(location.search).get('token') ?? ''
    void action.run(async () => { const response = await api.post<Envelope<Session>>('/v1/auth/email-verification/complete', { token }); signedIn(response.data); navigate('/', { replace: true }) })
    // The URL token defines this one-shot operation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, location.search])
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget), value = (name: string) => String(data.get(name) ?? '')
    await action.run(async () => {
      if (mode === 'forgot') { await api.post('/v1/auth/password-reset/request', { email: value('email') }); setNotice('If this account exists, reset instructions have been queued. Check your inbox and spam folder.'); return }
      if (mode === 'resend') { await api.post('/v1/auth/email-verification/resend', { email: value('email') }); setNotice('If this unverified account exists, a new link has been queued. Check your inbox and spam folder.'); return }
      if (mode === 'reset') { await api.post('/v1/auth/password-reset/complete', { token: new URLSearchParams(location.search).get('token') ?? '', password: value('password') }); window.dispatchEvent(new Event('mailer:session-expired')); setNotice('Password updated. You can now sign in.'); return }
      if (mode === 'signup') {
        if (config.result?.data.turnstileSiteKey && !turnstileToken) throw new Error('Complete the security check first.')
        await api.post('/v1/auth/signup', { email: value('email'), password: value('password'), first_name: value('first'), last_name: value('last'), turnstile_token: turnstileToken })
        setNotice('Account created. Check your inbox and spam folder for the verification link.'); return
      }
      const response = await api.post<Envelope<Session>>('/v1/auth/login', { email: value('email'), password: value('password'), remember: true }); signedIn(response.data); navigate('/', { replace: true })
    })
  }
  if (mode === 'verify') return <main className="live-auth"><div className="live-auth__brand"><Mail size={26} /><strong>CrescentSphere Mailer</strong></div><Panel title="Verify your email"><ErrorNotice error={action.error} />{action.busy && <p role="status">Verifying your account…</p>}{action.error && <button className="text-link" onClick={() => navigate('/login')}>Back to sign in</button>}</Panel></main>
  return <main className="live-auth"><div className="live-auth__brand"><Mail size={26} /><strong>CrescentSphere Mailer</strong><p>Developer email, under your control.</p></div><Panel title={mode === 'signup' ? 'Create your account' : mode === 'forgot' ? 'Recover your account' : mode === 'resend' ? 'Resend verification' : mode === 'reset' ? 'Set a new password' : 'Sign in'}><form key={mode} className="form-stack" onSubmit={submit}>
    {mode === 'signup' && <div className="form-two"><Field label="First name"><input name="first" autoComplete="given-name" required maxLength={80} /></Field><Field label="Last name"><input name="last" autoComplete="family-name" required maxLength={80} /></Field></div>}
    {mode !== 'reset' && <Field label="Email address"><input name="email" type="email" autoComplete="email" required maxLength={254} /></Field>}
    {!['forgot', 'resend'].includes(mode) && <Field label="Password"><input name="password" type="password" autoComplete={mode === 'login' ? 'current-password' : 'new-password'} minLength={mode === 'login' ? 1 : 12} maxLength={256} required /></Field>}
    {mode === 'signup' && <><label className="checkbox-field"><input type="checkbox" required />I will send only permission-based transactional email and handle bounces and complaints.</label><Turnstile siteKey={config.result?.data.turnstileSiteKey} token={setTurnstileToken} /></>}
    <ErrorNotice error={action.error || config.error} />{notice && <p className="live-notice" role="status">{notice}</p>}<Submit busy={action.busy}>{mode === 'signup' ? 'Create account' : mode === 'forgot' ? 'Send reset instructions' : mode === 'resend' ? 'Resend verification link' : mode === 'reset' ? 'Update password' : 'Sign in'}</Submit>
  </form><div className="live-actions"><button className="text-link" onClick={() => navigate(mode === 'login' ? '/signup' : '/login')}>{mode === 'login' ? 'Create account' : 'Back to sign in'}</button>{mode === 'login' && <><button className="text-link" onClick={() => navigate('/forgot-password')}>Forgot password?</button><button className="text-link" onClick={() => navigate('/resend-verification')}>Resend verification</button></>}</div></Panel><p className="muted">Public beta. New workspaces start in safe test mode.</p></main>
}
