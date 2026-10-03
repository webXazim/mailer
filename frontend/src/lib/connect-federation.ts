import { api } from './api/client'

const pendingKey = 'cs-mailer:connect-signin'
type ConnectConfig = { enabled: boolean; clientId?: string; authorizeUrl?: string; redirectUri?: string }

function randomToken() {
  const bytes = crypto.getRandomValues(new Uint8Array(32))
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

export async function beginConnectSignIn() {
  const response = await api.get<{ data: ConnectConfig }>('/v1/auth/connect/config')
  const config = response.data
  if (!config.enabled || !config.clientId || !config.authorizeUrl || !config.redirectUri) throw new Error('CS Connect sign-in is not configured yet.')
  const authorize = new URL(config.authorizeUrl)
  if (authorize.origin !== 'https://connect.crescentsphere.com') throw new Error('Invalid CS Connect sign-in address.')
  const redirect = new URL(config.redirectUri)
  if (redirect.origin !== location.origin || redirect.pathname !== '/auth/connect/callback') throw new Error('Invalid CS Mailer callback address.')
  const state = randomToken(), verifier = randomToken() + randomToken()
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier))
  const challenge = btoa(String.fromCharCode(...new Uint8Array(digest))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
  const requested = new URLSearchParams(location.search).get('return') || ''
  const next = requested.startsWith('/api/v1/auth/federation/authorize?') && !requested.includes('\\') ? requested : ''
  sessionStorage.setItem(pendingKey, JSON.stringify({ state, verifier, next, createdAt: Date.now() }))
  authorize.searchParams.set('response_type', 'code')
  authorize.searchParams.set('client_id', config.clientId)
  authorize.searchParams.set('redirect_uri', config.redirectUri)
  authorize.searchParams.set('scope', 'openid email profile')
  authorize.searchParams.set('state', state)
  authorize.searchParams.set('code_challenge', challenge)
  authorize.searchParams.set('code_challenge_method', 'S256')
  location.assign(authorize.toString())
}

export function consumeConnectCallback(search: string) {
  const params = new URLSearchParams(search)
  const raw = sessionStorage.getItem(pendingKey)
  sessionStorage.removeItem(pendingKey)
  history.replaceState(history.state, '', '/auth/connect/callback')
  if (!raw || params.has('error')) throw new Error('CS Connect sign-in was cancelled or expired.')
  let pending: { state: string; verifier: string; next?: string; createdAt: number }
  try { pending = JSON.parse(raw) } catch { throw new Error('Invalid CS Connect sign-in session.') }
  if (!params.get('code') || params.get('state') !== pending.state || Date.now() - pending.createdAt > 300_000) throw new Error('CS Connect sign-in was invalid or expired. Please try again.')
  const next = pending.next?.startsWith('/api/v1/auth/federation/authorize?') && !pending.next.includes('\\') ? pending.next : ''
  return { code: params.get('code')!, verifier: pending.verifier, next }
}
