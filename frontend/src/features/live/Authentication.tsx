import { FormEvent, useEffect, useRef, useState } from 'react'
import { ArrowLeft, Check, KeyRound, LockKeyhole, Mail, ShieldCheck, Sparkles } from 'lucide-react'
import { useLocation, useNavigate } from 'react-router-dom'
import { api, ApiError } from '../../lib/api/client'
import { BrandLogo, Envelope, ErrorNotice, Field, Notice, Session, Submit, useAction, useResource } from './shared'

type AuthConfig = { emailVerification: boolean; passwordRecovery: boolean; turnstileSiteKey?: string | null }
type SignupResult = { verificationRequired: boolean; verificationEmailStatus?: string; email: string; session?: Session }

declare global { interface Window { turnstile?: { render: (element: HTMLElement, options: Record<string, unknown>) => string; remove: (widget: string) => void } } }

function Turnstile({ siteKey, token }: { siteKey?: string | null; token: (value: string) => void }) {
  const container = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!siteKey) return
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
  return siteKey ? <div className="auth-turnstile"><div ref={container} aria-label="Security check" /></div> : null
}

export function Authentication({ signedIn }: { signedIn: (session: Session) => void }) {
  const location = useLocation(), navigate = useNavigate(), action = useAction()
  const mode = location.pathname === '/signup' ? 'signup' : location.pathname === '/forgot-password' ? 'forgot' : location.pathname === '/reset-password' ? 'reset' : location.pathname === '/verify-email' ? 'verify' : location.pathname === '/resend-verification' ? 'resend' : 'login'
  const config = useResource<AuthConfig>('/v1/auth/config', 60_000)
  const [notice, setNotice] = useState(''), [turnstileToken, setTurnstileToken] = useState('')
  useEffect(() => { setNotice(''); setTurnstileToken('') }, [mode])
  const authQuery = new URLSearchParams(location.search)
  const verificationEmail = authQuery.get('email') ?? ''
  const verificationQueued = authQuery.get('queued') === '1'

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget), value = (name: string) => String(data.get(name) ?? '')
    await action.run(async () => {
      if (mode === 'forgot') { await api.post('/v1/auth/password-reset/request', { email: value('email') }); setNotice('Request accepted. If this account exists, reset instructions were queued.'); return }
      if (mode === 'resend') { const email = value('email').trim().toLowerCase(); await api.post('/v1/auth/email-verification/resend', { email }); navigate(`/verify-email?email=${encodeURIComponent(email)}&queued=1`, { replace: true }); return }
      if (mode === 'verify') { const response = await api.post<Envelope<Session>>('/v1/auth/email-verification/complete', { email: value('email'), code: value('code') }); signedIn(response.data); navigate('/overview', { replace: true }); return }
      if (mode === 'reset') { await api.post('/v1/auth/password-reset/complete', { token: new URLSearchParams(location.search).get('token') ?? '', password: value('password') }); window.dispatchEvent(new Event('mailer:session-expired')); setNotice('Password updated. You can now sign in.'); return }
      if (mode === 'signup') {
        if (config.result?.data.turnstileSiteKey && !turnstileToken) throw new Error('Complete the security check first.')
        const response = await api.post<Envelope<SignupResult>>('/v1/auth/signup', { email: value('email'), password: value('password'), first_name: value('first'), last_name: value('last'), turnstile_token: turnstileToken })
        if (response.data.session) { signedIn(response.data.session); navigate('/overview', { replace: true }); return }
        navigate(`/verify-email?email=${encodeURIComponent(response.data.email)}&queued=1`, { replace: true }); return
      }
      const email = value('email').trim().toLowerCase()
      try {
        const response = await api.post<Envelope<Session>>('/v1/auth/login', { email, password: value('password'), remember: true }); signedIn(response.data); navigate('/overview', { replace: true })
      } catch (error) {
        if (error instanceof ApiError && error.body.code === 'email_not_verified') { navigate(`/verify-email?email=${encodeURIComponent(email)}`, { replace: true }); return }
        throw error
      }
    })
  }

  const title = mode === 'signup' ? 'Create your workspace' : mode === 'forgot' ? 'Reset your password' : mode === 'resend' ? 'Resend verification code' : mode === 'reset' ? 'Choose a new password' : mode === 'verify' ? 'Verify your email' : 'Welcome back'
  const description = mode === 'signup' ? 'Start in safe test mode and connect the API before enabling production delivery.' : mode === 'forgot' ? 'We will queue reset instructions for the address if an account exists.' : mode === 'resend' ? 'Enter the address you used when creating your account.' : mode === 'reset' ? 'Use at least 12 characters for your new password.' : mode === 'verify' ? 'Enter the six-digit code sent to your email address.' : 'Sign in to your CS Mailer developer console.'

  return <main className="auth-page">
    <aside className="auth-aside">
      <button className="brand-button auth-brand" onClick={() => navigate('/')}><BrandLogo /></button>
      <div className="auth-aside__visual" aria-hidden="true"><span className="orbit orbit--one" /><span className="orbit orbit--two" /><img src="/cs-mailer-logo.png" alt="" /></div>
      <div className="auth-aside__copy"><p className="public-kicker public-kicker--light">Developer email infrastructure</p><h1>Build the mail flow. See what happened next.</h1><p>Send transactional email, verify domains, inspect delivery, manage suppressions, and connect signed webhooks from one console.</p><ul><li><Check size={15} />Safe simulated test environment</li><li><Check size={15} />Environment-scoped API credentials</li><li><Check size={15} />Per-message delivery visibility</li></ul></div>
      <p className="auth-aside__foot"><ShieldCheck size={14} />Secure sessions · production safeguards · signed webhooks</p>
    </aside>

    <section className="auth-main">
      <div className="auth-card">
        <button className="auth-back" onClick={() => navigate('/')}><ArrowLeft size={14} />CS Mailer home</button>
        <header><span className="auth-icon">{mode === 'signup' ? <Sparkles size={20} /> : mode === 'reset' ? <LockKeyhole size={20} /> : mode === 'verify' ? <KeyRound size={20} /> : <Mail size={20} />}</span><p className="eyebrow">CS Mailer account</p><h2>{title}</h2><p>{description}</p></header>
        <form key={mode} className="form-stack auth-form" onSubmit={submit}>
          {mode === 'signup' && <div className="form-two"><Field label="First name"><input name="first" autoComplete="given-name" placeholder="Alex" required maxLength={80} /></Field><Field label="Last name"><input name="last" autoComplete="family-name" placeholder="Morgan" required maxLength={80} /></Field></div>}
          {mode !== 'reset' && <Field label="Email address"><input name="email" type="email" autoComplete="email" placeholder="you@company.com" defaultValue={['verify', 'resend'].includes(mode) ? verificationEmail : ''} required maxLength={254} /></Field>}
          {mode === 'verify' && <Field label="Verification code"><input className="auth-code" name="code" inputMode="numeric" autoComplete="one-time-code" placeholder="000000" pattern="[0-9]{6}" minLength={6} maxLength={6} required autoFocus /></Field>}
          {!['forgot', 'resend', 'verify'].includes(mode) && <Field label="Password" hint={mode === 'signup' ? 'Use 12 or more characters. A passphrase works well.' : undefined}><input name="password" type="password" autoComplete={mode === 'login' ? 'current-password' : 'new-password'} placeholder={mode === 'login' ? 'Enter your password' : 'At least 12 characters'} minLength={mode === 'login' ? 1 : 12} maxLength={256} required /></Field>}
          {mode === 'signup' && <><label className="checkbox-field auth-consent"><input type="checkbox" required /><span>I will use CS Mailer for permission-based transactional email and handle delivery failures responsibly.</span></label><Turnstile siteKey={config.result?.data.turnstileSiteKey} token={setTurnstileToken} /><p className="auth-legal">By creating an account you agree to the <button type="button" onClick={() => navigate('/terms')}>Terms of Service</button> and acknowledge the <button type="button" onClick={() => navigate('/privacy')}>Privacy Policy</button>.</p></>}
          {mode === 'verify' && verificationQueued && <Notice tone="info">Your account was created and the verification message was queued. Delivery is not yet confirmed.</Notice>}
          <ErrorNotice error={action.error || config.error} />{notice && <Notice tone="success">{notice}</Notice>}
          <Submit busy={action.busy} className="auth-submit">{mode === 'signup' ? 'Create workspace' : mode === 'forgot' ? 'Send reset instructions' : mode === 'resend' ? 'Resend verification code' : mode === 'verify' ? 'Verify account' : mode === 'reset' ? 'Update password' : 'Sign in'}</Submit>
        </form>
        <div className="auth-links"><button className="text-link" onClick={() => navigate(mode === 'login' ? '/signup' : mode === 'resend' && verificationEmail ? `/verify-email?email=${encodeURIComponent(verificationEmail)}` : '/login')}>{mode === 'login' ? 'Create a CS Mailer account' : mode === 'resend' && verificationEmail ? 'Back to code entry' : 'Back to sign in'}</button>{mode === 'login' && <>{config.result?.data.passwordRecovery && <button className="text-link" onClick={() => navigate('/forgot-password')}>Forgot password?</button>}{config.result?.data.emailVerification && <button className="text-link" onClick={() => navigate('/resend-verification')}>Resend verification</button>}</>}{mode === 'verify' && <button className="text-link" onClick={() => navigate(`/resend-verification?email=${encodeURIComponent(verificationEmail)}`)}>Send a new code</button>}</div>
      </div>
      <p className="auth-main__footnote">New workspaces begin in test mode · Verify a sender domain to unlock production</p>
    </section>
  </main>
}
