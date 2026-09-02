import { FormEvent, useEffect, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { Check, Mail, ShieldCheck, Zap } from 'lucide-react'
import { api } from '../../lib/api/client'
import { Envelope, Session, useAction, useResource, Field, ErrorNotice, Submit } from './shared'

type AuthConfig = { emailVerification: boolean; passwordRecovery: boolean; turnstileSiteKey?: string }
type SignupResult = { verificationRequired: boolean; email: string; session?: Session }
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
        const response = await api.post<Envelope<SignupResult>>('/v1/auth/signup', { email: value('email'), password: value('password'), first_name: value('first'), last_name: value('last'), turnstile_token: turnstileToken })
        if (response.data.session) { signedIn(response.data.session); navigate('/', { replace: true }); return }
        setNotice('Account created. Check your inbox and spam folder for the verification link.'); return
      }
      const response = await api.post<Envelope<Session>>('/v1/auth/login', { email: value('email'), password: value('password'), remember: true }); signedIn(response.data); navigate('/', { replace: true })
    })
  }
  const title = mode === 'signup' ? 'Create your account' : mode === 'forgot' ? 'Reset your password' : mode === 'resend' ? 'Resend verification' : mode === 'reset' ? 'Choose a new password' : 'Welcome back'
  const description = mode === 'signup' ? 'Start testing your email integration in a few minutes.' : mode === 'forgot' ? 'We will send a secure reset link to your inbox.' : mode === 'resend' ? 'Enter the address you used when creating your account.' : mode === 'reset' ? 'Use at least 12 characters for your new password.' : 'Sign in to manage your email infrastructure.'
  return <main className="mailer-auth">
    <aside className="mailer-auth__aside">
      <div className="mailer-auth__brand"><span><Mail size={19} /></span>CrescentSphere Mailer</div>
      <div className="mailer-auth__pitch"><p className="eyebrow">Developer email infrastructure</p><h1>Ship transactional email with confidence.</h1><p>One focused console for sending, delivery events, domains, suppressions, and webhooks.</p><ul><li><Check size={15} />Safe test mode with simulated delivery</li><li><Check size={15} />SES-backed production sending</li><li><Check size={15} />Signed webhooks and delivery history</li></ul></div>
      <p className="mailer-auth__aside-footer"><ShieldCheck size={14} />Protected by Cloudflare and secure sessions</p>
    </aside>
    <section className="mailer-auth__main">
      <div className="mailer-auth__card">
        <header><span className="mailer-auth__icon">{mode === 'signup' ? <Zap size={19} /> : <Mail size={19} />}</span><h2>{mode === 'verify' ? 'Verify your email' : title}</h2><p>{mode === 'verify' ? 'We are confirming your secure verification link.' : description}</p></header>
        {mode === 'verify' ? <div className="mailer-auth__status"><ErrorNotice error={action.error} />{action.busy && <p role="status">Verifying your account…</p>}{action.error && <button className="text-link" onClick={() => navigate('/login')}>Back to sign in</button>}</div> : <>
          <form key={mode} className="form-stack mailer-auth__form" onSubmit={submit}>
            {mode === 'signup' && <div className="form-two"><Field label="First name"><input name="first" autoComplete="given-name" placeholder="Alex" required maxLength={80} /></Field><Field label="Last name"><input name="last" autoComplete="family-name" placeholder="Morgan" required maxLength={80} /></Field></div>}
            {mode !== 'reset' && <Field label="Email address"><input name="email" type="email" autoComplete="email" placeholder="you@company.com" required maxLength={254} /></Field>}
            {!['forgot', 'resend'].includes(mode) && <Field label="Password"><input name="password" type="password" autoComplete={mode === 'login' ? 'current-password' : 'new-password'} placeholder={mode === 'login' ? 'Enter your password' : 'At least 12 characters'} minLength={mode === 'login' ? 1 : 12} maxLength={256} required />{mode === 'signup' && <small>Use 12 or more characters. A passphrase works well.</small>}</Field>}
            {mode === 'signup' && <><label className="checkbox-field mailer-auth__consent"><input type="checkbox" required /><span>I will send only permission-based transactional email and handle bounces and complaints.</span></label><Turnstile siteKey={config.result?.data.turnstileSiteKey} token={setTurnstileToken} /></>}
            <ErrorNotice error={action.error || config.error} />{notice && <p className="live-notice" role="status">{notice}</p>}<Submit busy={action.busy}>{mode === 'signup' ? 'Create account' : mode === 'forgot' ? 'Send reset instructions' : mode === 'resend' ? 'Resend verification link' : mode === 'reset' ? 'Update password' : 'Sign in'}</Submit>
          </form>
          <div className="mailer-auth__links"><button className="text-link" onClick={() => navigate(mode === 'login' ? '/signup' : '/login')}>{mode === 'login' ? 'Create a free account' : 'Back to sign in'}</button>{mode === 'login' && <>{config.result?.data.passwordRecovery && <button className="text-link" onClick={() => navigate('/forgot-password')}>Forgot password?</button>}{config.result?.data.emailVerification && <button className="text-link" onClick={() => navigate('/resend-verification')}>Resend verification</button>}</>}</div>
        </>}
      </div>
      <p className="mailer-auth__footnote">Public beta · New workspaces begin in safe test mode</p>
    </section>
  </main>
}
