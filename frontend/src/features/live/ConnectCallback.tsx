import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { api } from '../../lib/api/client'
import { consumeConnectCallback } from '../../lib/connect-federation'
import { BrandLogo, Envelope, Session } from './shared'

export function ConnectCallback({ signedIn }: { signedIn: (session: Session) => void }) {
  const navigate = useNavigate(), started = useRef(false)
  const [error, setError] = useState('')
  useEffect(() => {
    if (started.current) return
    started.current = true
    void Promise.resolve().then(async () => {
      const callback = consumeConnectCallback(location.search)
      const response = await api.post<Envelope<Session>>('/v1/auth/connect/complete', { code: callback.code, code_verifier: callback.verifier })
      return { response, next: callback.next }
    })
      .then(({ response, next }) => { signedIn(response.data); if (next) location.replace(next); else navigate('/overview', { replace: true }) })
      .catch((cause) => setError(cause instanceof Error ? cause.message : 'Unable to complete sign-in.'))
  }, [navigate, signedIn])
  return <main className="auth-page"><section className="auth-main"><div className="auth-card"><BrandLogo /><h1>Completing CS Connect sign-in</h1>{error ? <><p role="alert">{error}</p><button className="text-link" onClick={() => navigate('/login')}>Back to sign in</button></> : <p role="status">Opening your CS Mailer account…</p>}</div></section></main>
}
