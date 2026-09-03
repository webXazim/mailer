import { FormEvent, useEffect, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { Check, Mail, ShieldCheck, Zap } from 'lucide-react'
import { api, ApiError } from '../../lib/api/client'
import { Envelope, Session, useAction, useResource, Field, ErrorNotice, Submit } from './shared'

type AuthConfig = { emailVerification: boolean; passwordRecovery: boolean; turnstileSiteKey?: string }
type SignupResult = { verificationRequired: boolean; verificationEmailStatus?: 'queued'; email: string; session?: Session }
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
  return siteKey ? <div className="mailer-auth__turnstile"><div ref={container} aria-label="Security check" /></div> : null
}

export function Authentication({ signedIn }: { signedIn: (session: Session) => void }) {
  const location = useLocation(), navigate = useNavigate(), action = useAction()
  const mode = location.pathname === '/signup' ? 'signup' : location.pathname === '/forgot-password' ? 'forgot' : location.pathname === '/reset-password' ? 'reset' : location.pathname === '/verify-email' ? 'verify' : location.pathname === '/resend-verification' ? 'resend' : 'login'
  const config = useResource<AuthConfig>('/v1/auth/config')
  const [notice, setNotice] = useState(''), [turnstileToken, setTurnstileToken] = useState('')
  useEffect(() => { setNotice(''); setTurnstileToken('') }, [mode])
  const authQuery = new URLSearchParams(location.search)
  const verificationEmail = authQuery.get('email') ?? ''
  const verificationQueued = authQuery.get('queued') === '1'
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget), value = (name: string) => String(data.get(name) ?? '')
    await action.run(async () => {
      if (mode === 'forgot') { await api.post('/v1/auth/password-reset/request', { email: value('email') }); setNotice('Request accepted. If this account exists, reset instructions were queued. Delivery is not yet confirmed.'); return }
      if (mode === 'resend') { const email = value('email').trim().toLowerCase(); await api.post('/v1/auth/email-verification/resend', { email }); navigate(`/verify-email?email=${encodeURIComponent(email)}&queued=1`, { replace: true }); return }
      if (mode === 'verify') { const response = await api.post<Envelope<Session>>('/v1/auth/email-verification/complete', { email: value('email'), code: value('code') }); signedIn(response.data); navigate('/', { replace: true }); return }
      if (mode === 'reset') { await api.post('/v1/auth/password-reset/complete', { token: new URLSearchParams(location.search).get('token') ?? '', password: value('password') }); window.dispatchEvent(new Event('mailer:session-expired')); setNotice('Password updated. You can now sign in.'); return }
      if (mode === 'signup') {
        if (config.result?.data.turnstileSiteKey && !turnstileToken) throw new Error('Complete the security check first.')
        const response = await api.post<Envelope<SignupResult>>('/v1/auth/signup', { email: value('email'), password: value('password'), first_name: value('first'), last_name: value('last'), turnstile_token: turnstileToken })
        if (response.data.session) { signedIn(response.data.session); navigate('/', { replace: true }); return }
        navigate(`/verify-email?email=${encodeURIComponent(response.data.email)}&queued=1`, { replace: true }); return
      }
      const email = value('email').trim().toLowerCase()
      try {
        const response = await api.post<Envelope<Session>>('/v1/auth/login', { email, password: value('password'), remember: true }); signedIn(response.data); navigate('/', { replace: true })
      } catch (error) {
        if (error instanceof ApiError && error.body.code === 'email_not_verified') { navigate(`/verify-email?email=${encodeURIComponent(email)}`, { replace: true }); return }
        throw error
      }
    })
  }
  const title = mode === 'signup' ? 'Create your account' : mode === 'forgot' ? 'Reset your password' : mode === 'resend' ? 'Resend verification code' : mode === 'reset' ? 'Choose a new password' : 'Welcome back'
  const description = mode === 'signup' ? 'Start testing your email integration in a few minutes.' : mode === 'forgot' ? 'We will send a secure reset link to your inbox.' : mode === 'resend' ? 'Enter the address you used when creating your account.' : mode === 'reset' ? 'Use at least 12 characters for your new password.' : 'Sign in to manage your email infrastructure.'
  return <main className="mailer-auth">
    <aside className="mailer-auth__aside">
      <div className="mailer-auth__brand"><span><img src="/crescentsphere-mark.svg" alt="" /></span>CrescentSphere Mailer</div>
      <div className="mailer-auth__pitch"><p className="eyebrow">Developer email infrastructure</p><h1>Ship transactional email with confidence.</h1><p>One focused console for sending, delivery events, domains, suppressions, and webhooks.</p><ul><li><Check size={15} />Safe test mode with simulated delivery</li><li><Check size={15} />SES-backed production sending</li><li><Check size={15} />Signed webhooks and delivery history</li></ul></div>
      <p className="mailer-auth__aside-footer"><ShieldCheck size={14} />Protected by Cloudflare and secure sessions</p>
    </aside>
    <section className="mailer-auth__main">
      <div className="mailer-auth__card">
        <header><span className="mailer-auth__icon">{mode === 'signup' ? <Zap size={19} /> : <Mail size={19} />}</span><h2>{mode === 'verify' ? 'Check your email' : title}</h2><p>{mode === 'verify' ? 'Enter the six-digit verification code from your email.' : description}</p></header>
        <>
          <form key={mode} className="form-stack mailer-auth__form" onSubmit={submit}>
            {mode === 'signup' && <div className="form-two"><Field label="First name"><input name="first" autoComplete="given-name" placeholder="Alex" required maxLength={80} /></Field><Field label="Last name"><input name="last" autoComplete="family-name" placeholder="Morgan" required maxLength={80} /></Field></div>}
            {mode !== 'reset' && <Field label="Email address"><input name="email" type="email" autoComplete="email" placeholder="you@company.com" defaultValue={['verify', 'resend'].includes(mode) ? verificationEmail : ''} required maxLength={254} /></Field>}
            {mode === 'verify' && <Field label="Verification code"><input className="mailer-auth__code" name="code" inputMode="numeric" autoComplete="one-time-code" placeholder="000000" pattern="[0-9]{6}" minLength={6} maxLength={6} required autoFocus /></Field>}
            {!['forgot', 'resend', 'verify'].includes(mode) && <Field label="Password"><input name="password" type="password" autoComplete={mode === 'login' ? 'current-password' : 'new-password'} placeholder={mode === 'login' ? 'Enter your password' : 'At least 12 characters'} minLength={mode === 'login' ? 1 : 12} maxLength={256} required />{mode === 'signup' && <small>Use 12 or more characters. A passphrase works well.</small>}</Field>}
            {mode === 'signup' && <><label className="checkbox-field mailer-auth__consent"><input type="checkbox" required /><span>I will send only permission-based transactional email and handle bounces and complaints.</span></label><Turnstile siteKey={config.result?.data.turnstileSiteKey} token={setTurnstileToken} /></>}
            {mode === 'verify' && verificationQueued && <p className="live-notice live-notice--pending" role="status">Your account was created and the verification email was queued. Delivery is not yet confirmed.</p>}
            <ErrorNotice error={action.error || config.error} />{notice && <p className={`live-notice ${['signup', 'forgot', 'resend'].includes(mode) ? 'live-notice--pending' : ''}`} role="status">{notice}</p>}<Submit busy={action.busy}>{mode === 'signup' ? 'Create account' : mode === 'forgot' ? 'Send reset instructions' : mode === 'resend' ? 'Resend verification code' : mode === 'verify' ? 'Verify account' : mode === 'reset' ? 'Update password' : 'Sign in'}</Submit>
          </form>
          <div className="mailer-auth__links"><button className="text-link" onClick={() => navigate(mode === 'login' ? '/signup' : mode === 'resend' && verificationEmail ? `/verify-email?email=${encodeURIComponent(verificationEmail)}` : '/login')}>{mode === 'login' ? 'Create a free account' : mode === 'resend' && verificationEmail ? 'Back to code entry' : 'Back to sign in'}</button>{mode === 'login' && <>{config.result?.data.passwordRecovery && <button className="text-link" onClick={() => navigate('/forgot-password')}>Forgot password?</button>}{config.result?.data.emailVerification && <button className="text-link" onClick={() => navigate('/resend-verification')}>Resend verification</button>}</>}{mode === 'verify' && <button className="text-link" onClick={() => navigate(`/resend-verification?email=${encodeURIComponent(verificationEmail)}`)}>Send a new code</button>}</div>
        </>
      </div>
      <p className="mailer-auth__footnote">New workspaces begin in safe test mode · Verify a domain to send in production</p>
    </section>
  </main>
}
