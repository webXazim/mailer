import { ArrowRight, Braces, Check, Code2, Globe2, KeyRound, MailCheck, Route, ShieldCheck, Webhook, Zap } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { BrandLogo } from './shared'

export function LandingPage({ signedIn, signIn, createAccount }: { signedIn: boolean; signIn: () => void; createAccount: () => void }) {
  const navigate = useNavigate()
  const scroll = (id: string) => document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  return <main className="public-home">
    <header className="public-nav">
      <button className="brand-button" onClick={() => window.scrollTo({ top: 0, behavior: 'smooth' })} aria-label="CS Mailer home"><BrandLogo /></button>
      <nav aria-label="Public navigation">
        <button onClick={() => scroll('platform')}>Product</button>
        <button onClick={() => scroll('developers')}>Developers</button>
        <button onClick={() => navigate('/privacy')}>Privacy</button>
        <button onClick={() => navigate('/terms')}>Terms</button>
        {!signedIn && <button onClick={signIn}>Sign in</button>}
        <button className="public-nav__cta" onClick={createAccount}>{signedIn ? 'Open console' : 'Start testing'} <ArrowRight size={14} /></button>
      </nav>
    </header>

    <section className="public-hero">
      <div className="public-hero__watermark" aria-hidden="true"><img src="/cs-mailer-logo.png" alt="" /></div>
      <div className="public-hero__copy">
        <p className="public-kicker"><span />Developer email infrastructure</p>
        <h1>Transactional email, built to feel like an API.</h1>
        <p className="public-hero__lead">CS Mailer gives application teams one focused place to send transactional email, verify sender domains, observe delivery, manage suppressions, and connect signed webhooks.</p>
        <div className="public-hero__actions">
          <button className="public-button public-button--primary" onClick={createAccount}>{signedIn ? 'Open CS Mailer' : 'Create a workspace'} <ArrowRight size={17} /></button>
          <button className="public-button public-button--secondary" onClick={() => scroll('developers')}><Code2 size={16} />Explore the API</button>
        </div>
        <ul className="public-proof" aria-label="Platform highlights">
          <li><Check size={14} />Safe simulated test environment</li>
          <li><Check size={14} />Verified production sender domains</li>
          <li><Check size={14} />Signed delivery webhooks</li>
        </ul>
      </div>
      <div className="public-hero__demo" aria-label="Example CS Mailer API request">
        <div className="terminal-bar"><div><span /><span /><span /></div><small>POST /api/v1/emails</small></div>
        <div className="terminal-tabs"><span className="is-active">cURL</span><span>Node.js</span><span>Python</span></div>
        <pre><code>{`curl https://mailer.example.com/api/v1/emails \\
  -H "Authorization: Bearer $CS_MAILER_KEY" \\
  -H "Idempotency-Key: order-4821-receipt" \\
  -H "Content-Type: application/json" \\
  -d '{
    "from": "Acme <hello@mail.acme.com>",
    "to": ["customer@example.com"],
    "subject": "Your receipt",
    "text": "Thanks for your order."
  }'`}</code></pre>
        <div className="terminal-result"><span><MailCheck size={17} />202 Accepted</span><code>queued → sent → delivered</code></div>
      </div>
    </section>

    <section className="public-signal-strip" aria-label="CS Mailer workflow">
      <span><Braces size={16} />One send API</span><span><Globe2 size={16} />DNS verification</span><span><Webhook size={16} />Signed events</span><span><ShieldCheck size={16} />Suppression controls</span><span><KeyRound size={16} />Scoped API keys</span>
    </section>

    <section className="public-section" id="platform">
      <div className="public-section__heading"><p className="public-kicker">The platform</p><h2>Everything around delivery, without hiding the delivery state.</h2><p>Your application sends one request. CS Mailer keeps the operational detail visible so developers can distinguish accepted, queued, sent, delivered, bounced, complained, and failed outcomes.</p></div>
      <div className="capability-grid">
        <article className="capability capability--blue"><span><Zap size={19} /></span><h3>Send API</h3><p>Submit transactional mail with idempotency protection, multiple recipients, metadata, and attachments.</p></article>
        <article className="capability capability--cyan"><span><Globe2 size={19} /></span><h3>Domain readiness</h3><p>Provision sender domains, publish required DNS records, and follow verification status from the console.</p></article>
        <article className="capability capability--violet"><span><Webhook size={19} /></span><h3>Delivery webhooks</h3><p>Subscribe endpoints to delivery, bounce, complaint, reject, rendering, open, and click events.</p></article>
        <article className="capability capability--green"><span><ShieldCheck size={19} /></span><h3>Reputation controls</h3><p>Keep recipient suppressions explicit and separate test traffic from production sending.</p></article>
      </div>
    </section>

    <section className="public-developer" id="developers">
      <div className="public-developer__copy"><p className="public-kicker">Developer workflow</p><h2>From first request to production in a predictable sequence.</h2><p>The frontend keeps the test and production environments visually distinct while the API contract stays stable.</p><ol><li><b>01</b><div><strong>Create a test key</strong><span>Integrate without sending recipient email.</span></div></li><li><b>02</b><div><strong>Verify a sender domain</strong><span>Publish the generated DNS records and monitor validation.</span></div></li><li><b>03</b><div><strong>Create a production key</strong><span>Use scoped credentials only from trusted server code.</span></div></li><li><b>04</b><div><strong>Observe final outcomes</strong><span>Use the console and signed webhooks for delivery evidence.</span></div></li></ol></div>
      <div className="public-developer__panel"><div className="developer-panel__head"><Route size={18} /><span>Delivery lifecycle</span><i>LIVE</i></div><div className="delivery-path"><div className="is-done"><span>1</span><strong>Accepted</strong><small>API request validated</small></div><div className="is-done"><span>2</span><strong>Queued</strong><small>Message stored for delivery</small></div><div className="is-done"><span>3</span><strong>Sent</strong><small>Transport accepted message</small></div><div className="is-live"><span>4</span><strong>Delivered</strong><small>Recipient server accepted</small></div></div><div className="developer-panel__footer"><Code2 size={16} /><span>Use <code>GET /v1/emails/{'{id}'}</code> or signed webhooks for final state.</span></div></div>
    </section>

    <section className="public-control">
      <div><p className="public-kicker">Operational clarity</p><h2>A mail platform your developers can reason about.</h2><p>CS Mailer separates API acceptance from actual delivery, exposes provider attempts and recipient states, and keeps production controls visible instead of treating email as a fire-and-forget call.</p><button className="public-button public-button--light" onClick={createAccount}>{signedIn ? 'Open your console' : 'Start in test mode'} <ArrowRight size={17} /></button></div>
      <div className="public-control__list"><p><Code2 size={18} /><span><strong>Stable integration</strong>Keep transport-specific operations out of application code.</span></p><p><MailCheck size={18} /><span><strong>Honest status</strong>See the difference between queued, sent, and delivered.</span></p><p><ShieldCheck size={18} /><span><strong>Safer credentials</strong>Use environment-specific API keys with explicit scopes.</span></p></div>
    </section>

    <section className="public-final-cta"><div><p className="public-kicker public-kicker--light">Start with simulated delivery</p><h2>Build the integration before you touch a real inbox.</h2><p>Create a workspace, issue a test key, and validate your first transactional flow.</p></div><button className="public-button public-button--light" onClick={createAccount}>{signedIn ? 'Open CS Mailer' : 'Create account'} <ArrowRight size={17} /></button></section>

    <footer className="public-footer"><div className="public-footer__brand"><BrandLogo /><p>Developer email infrastructure for transactional application mail.</p></div><div><strong>Platform</strong><button onClick={() => scroll('platform')}>Product</button><button onClick={() => scroll('developers')}>Developers</button>{!signedIn && <button onClick={signIn}>Sign in</button>}</div><div><strong>Legal</strong><button onClick={() => navigate('/privacy')}>Privacy policy</button><button onClick={() => navigate('/terms')}>Terms of service</button></div><div className="public-footer__meta"><span>© {new Date().getFullYear()} CrescentSphere</span><span>CS Mailer</span></div></footer>
  </main>
}
