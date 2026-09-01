import { FormEvent, useEffect, useState } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { Mail } from 'lucide-react'
import { api } from '../../lib/api/client'
import { Envelope, Session, useAction, useResource, Panel, Field, ErrorNotice, Submit } from './shared'
export function Authentication({ signedIn }: { signedIn: (session: Session) => void }) {
  const location = useLocation(), navigate = useNavigate(), action = useAction()
  const mode = location.pathname === '/signup' ? 'signup' : location.pathname === '/forgot-password' ? 'forgot' : location.pathname === '/reset-password' ? 'reset' : 'login'
  const config = useResource<{ inviteRequired: boolean; passwordRecovery: boolean }>('/v1/auth/config')
  const [notice, setNotice] = useState('')
  useEffect(() => setNotice(''), [mode])
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget), value = (name: string) => String(data.get(name) ?? '')
    await action.run(async () => {
      if (mode === 'forgot') {
        await api.post('/v1/auth/password-reset/request', { email: value('email') })
        setNotice('If this account exists, reset instructions have been queued. Check your inbox and spam folder.'); return
      }
      if (mode === 'reset') {
        await api.post('/v1/auth/password-reset/complete', { token: new URLSearchParams(location.search).get('token') ?? '', password: value('password') })
        window.dispatchEvent(new Event('mailer:session-expired')); setNotice('Password updated. You can now sign in.'); return
      }
      const body = mode === 'signup' ? { email: value('email'), password: value('password'), first_name: value('first'), last_name: value('last'), workspace_name: value('workspace'), signup_token: value('invite') } : { email: value('email'), password: value('password'), remember: true }
      const response = await api.post<Envelope<Session>>(`/v1/auth/${mode}`, body)
      signedIn(response.data); navigate('/', { replace: true })
    })
  }
  return <main className="live-auth"><div className="live-auth__brand"><Mail size={26} /><strong>CrescentSphere Mailer</strong><p>Developer email, under your control.</p></div><Panel title={mode === 'signup' ? 'Create your private workspace' : mode === 'forgot' ? 'Recover your account' : mode === 'reset' ? 'Set a new password' : 'Sign in'}><form key={mode} className="form-stack" onSubmit={submit}>
    {mode === 'signup' && <><div className="form-two"><Field label="First name"><input name="first" autoComplete="given-name" required /></Field><Field label="Last name"><input name="last" autoComplete="family-name" required /></Field></div><Field label="Workspace name"><input name="workspace" maxLength={80} required /></Field><Field label="Private signup token"><input name="invite" type="password" autoComplete="off" required={config.result?.data.inviteRequired ?? true} /><small>Provided by the operator from SIGNUP_TOKEN in the server .env.</small></Field></>}
    {mode !== 'reset' && <Field label="Email address"><input name="email" type="email" autoComplete="email" required maxLength={254} /></Field>}
    {mode !== 'forgot' && <Field label="Password"><input name="password" type="password" autoComplete={mode === 'login' ? 'current-password' : 'new-password'} minLength={mode === 'login' ? 1 : 12} maxLength={256} required /></Field>}
    <ErrorNotice error={action.error || config.error} />{notice && <p className="live-notice" role="status">{notice}</p>}<Submit busy={action.busy}>{mode === 'signup' ? 'Create workspace' : mode === 'forgot' ? 'Send reset instructions' : mode === 'reset' ? 'Update password' : 'Sign in'}</Submit>
  </form><div className="live-actions"><button className="text-link" onClick={() => navigate(mode === 'login' ? '/signup' : '/login')}>{mode === 'login' ? 'Create workspace' : 'Back to sign in'}</button>{mode === 'login' && <button className="text-link" onClick={() => navigate('/forgot-password')}>Forgot password?</button>}</div></Panel><p className="muted">Private release. Production mail requires a verified sending domain.</p></main>
}
