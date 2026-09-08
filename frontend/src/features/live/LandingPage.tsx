import { ArrowRight, Check, Code2, Globe2, MailCheck, Route, ShieldCheck, Webhook } from 'lucide-react'

export function LandingPage({ signedIn, signIn, createAccount }: { signedIn: boolean; signIn: () => void; createAccount: () => void }) {
  const scrollToHowItWorks = () => document.getElementById('how-it-works')?.scrollIntoView({ behavior: 'smooth' })
  return <main className="public-home">
    <header className="public-nav">
      <button className="public-brand" onClick={() => window.scrollTo({ top: 0, behavior: 'smooth' })} aria-label="CrescentSphere Mailer home">
        <span><img src="/crescentsphere-mark.svg" alt="" /></span>
        <strong>CrescentSphere <b>Mailer</b></strong>
      </button>
      <nav aria-label="Public navigation">
        <button onClick={scrollToHowItWorks}>How it works</button>
        {!signedIn && <button onClick={signIn}>Sign in</button>}
        <button className="public-nav__cta" onClick={createAccount}>{signedIn ? 'Open console' : 'Start testing'}</button>
      </nav>
    </header>

    <section className="public-hero">
      <div className="public-hero__copy">
        <p className="public-kicker"><span />Transactional email infrastructure</p>
        <h1>Send application email with delivery you can see.</h1>
        <p className="public-hero__lead">One API for verification codes, password resets, receipts, and product notifications. Test safely, verify your domain, then send through our independently operated SMTP infrastructure.</p>
        <div className="public-hero__actions">
          <button className="public-button public-button--primary" onClick={createAccount}>{signedIn ? 'Open your console' : 'Create your workspace'} <ArrowRight size={17} /></button>
          <button className="public-button public-button--secondary" onClick={scrollToHowItWorks}>See how it works</button>
        </div>
        <ul className="public-proof" aria-label="Platform highlights">
          <li><Check size={14} />Simulated test mode</li>
          <li><Check size={14} />Verified sending domains</li>
          <li><Check size={14} />Signed delivery webhooks</li>
        </ul>
      </div>
      <div className="public-hero__demo" aria-label="Example email delivery request">
        <div className="public-demo__top"><span /><span /><span /><small>POST /api/v1/emails</small></div>
        <pre>{`{
  "from": "Acme <no-reply@mail.acme.com>",
  "to": ["customer@example.com"],
  "subject": "Your account is ready",
  "text": "Welcome to Acme."
}`}</pre>
        <div className="public-demo__result"><span><MailCheck size={17} /> delivery.delivered</span><code>250 OK · gmail-smtp-in</code></div>
      </div>
    </section>

    <section className="public-capabilities" aria-label="Mailer capabilities">
      <article><ShieldCheck size={20} /><h2>Safe by default</h2><p>Test keys simulate delivery. Production access starts after sender-domain verification.</p></article>
      <article><Route size={20} /><h2>Independent delivery</h2><p>Use authenticated SMTP with one stable API contract and status model.</p></article>
      <article><Webhook size={20} /><h2>Final outcomes</h2><p>Track queued, sent, delivered, bounced, and complained states through signed webhooks.</p></article>
      <article><Globe2 size={20} /><h2>Domain controls</h2><p>Provision DKIM, verify DNS, align return paths, and protect reputation with suppressions.</p></article>
    </section>

    <section className="public-process" id="how-it-works">
      <div className="public-section-heading"><p className="public-kicker">From code to inbox</p><h2>A clear path from first request to production.</h2><p>The console keeps testing separate from real delivery while your integration uses the same endpoint and payload.</p></div>
      <ol>
        <li><span>01</span><div><h3>Create a test key</h3><p>Build and validate your integration without sending mail to recipients.</p></div></li>
        <li><span>02</span><div><h3>Verify your domain</h3><p>Publish the required DNS records and confirm your sender identity.</p></div></li>
        <li><span>03</span><div><h3>Send in production</h3><p>Use a production key and follow each recipient through its final delivery outcome.</p></div></li>
      </ol>
    </section>

    <section className="public-control">
      <div><p className="public-kicker">Built for application teams</p><h2>Operational controls stay behind a simple send API.</h2><p>Idempotent submissions prevent accidental duplicates. Per-recipient status, retry-safe webhooks, suppression handling, and provider attempt history give operators the evidence they need when delivery fails.</p><button className="public-button public-button--primary" onClick={createAccount}>{signedIn ? 'Open your console' : 'Start in test mode'} <ArrowRight size={17} /></button></div>
      <div className="public-control__list">
        <p><Code2 size={18} /><span><strong>Stable integration</strong>Keep provider switching out of application code.</span></p>
        <p><MailCheck size={18} /><span><strong>Honest status</strong>“Sent” means provider accepted; “delivered” means the recipient server accepted.</span></p>
        <p><ShieldCheck size={18} /><span><strong>Production safeguards</strong>Authentication, quotas, rate limits, signed events, and abuse containment.</span></p>
      </div>
    </section>

    <section className="public-final-cta"><div><h2>Build your first email flow today.</h2><p>Create a workspace, issue a test key, and send your first simulated request in minutes.</p></div><button className="public-button public-button--light" onClick={createAccount}>{signedIn ? 'Open console' : 'Create free account'} <ArrowRight size={17} /></button></section>
    <footer className="public-footer"><span>CrescentSphere Mailer</span><span>Developer email infrastructure</span><button onClick={signedIn ? createAccount : signIn}>{signedIn ? 'Open console' : 'Sign in'}</button></footer>
  </main>
}
