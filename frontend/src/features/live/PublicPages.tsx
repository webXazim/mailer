import { ArrowLeft, ExternalLink, LockKeyhole, Mail, Server, ShieldCheck, Webhook } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { BrandLogo } from './shared'

const sections: Record<'terms' | 'privacy', Array<[string, string]>> = {
  terms: [
    ['1. Service', 'CS Mailer provides developer-facing infrastructure for submitting transactional email, configuring sending domains, managing API credentials, observing message activity, receiving webhook events, and managing recipient suppressions. Features available to an account may depend on its deployed configuration.'],
    ['2. Accounts and credentials', 'You are responsible for keeping account credentials, API keys, webhook secrets, and other authentication material secure. API keys are intended for trusted server-side use and should not be embedded in public browser or mobile application code.'],
    ['3. Permitted email', 'Use CS Mailer only for lawful, permission-based application email. You must have a valid basis to contact recipients and must not use the service for unsolicited bulk messaging, deceptive content, phishing, malware distribution, or attempts to evade recipient protections.'],
    ['4. Domains and sender identity', 'You may connect only domains and sender identities that you are authorized to use. Domain verification and production access are safeguards; they do not transfer ownership or authorization over a domain.'],
    ['5. Delivery and third-party systems', 'Email delivery depends on recipient servers, DNS, networks, and other systems outside CS Mailer. An accepted or sent status does not guarantee inbox placement. The console distinguishes message states so application teams can observe final outcomes when available.'],
    ['6. Webhooks and integrations', 'Webhook endpoints must be controlled by you and able to receive signed event deliveries safely. Receivers should validate signatures, tolerate retries, and process duplicate deliveries idempotently.'],
    ['7. Abuse and service protection', 'Access may be restricted, paused, or disabled when needed to protect recipients, platform security, sending reputation, or service availability, including suspected abuse or compromised credentials.'],
    ['8. Your content and recipients', 'You remain responsible for the email content, recipient addresses, metadata, attachments, and other information you submit through the service, including ensuring that your use complies with applicable law and your own privacy obligations.'],
    ['9. Availability and changes', 'The service may evolve as operational and security requirements change. Unless a separate written agreement says otherwise, product documentation and the deployed interface describe the currently available functionality.'],
    ['10. Contact', 'Questions about these terms or the operation of CS Mailer should be directed through the support or administrative contact provided with your CS Mailer deployment.']
  ],
  privacy: [
    ['1. Scope', 'This policy describes information processed when you use the CS Mailer website, developer console, authentication flows, API, domain tools, email activity views, webhook configuration, and suppression controls.'],
    ['2. Account information', 'CS Mailer processes account details such as name and email address so users can authenticate, access the developer console, and receive account-related messages such as verification or password-reset instructions when those features are enabled.'],
    ['3. Developer and service data', 'The service processes account settings, API-key metadata, sending-domain configuration, DNS verification status, webhook endpoint configuration, suppression entries, and operational usage counters needed to provide the console and enforce account controls. Secret credential values are intended to be shown only when created or rotated.'],
    ['4. Email submission data', 'When you submit an email, the service may process sender and recipient addresses, subject, message content, metadata, attachments, delivery status, errors, and delivery events. Content availability may be limited by the service retention configuration.'],
    ['5. Delivery and webhook records', 'Operational records may include recipient delivery state, delivery attempts, message identifiers, webhook delivery attempts, response status codes, retry timing, and error information. These records support troubleshooting and delivery visibility.'],
    ['6. Security and access data', 'The service may process session identifiers, authentication events, rate-limit and abuse-control signals, request metadata, and server logs needed to secure accounts, protect the platform, diagnose failures, and investigate misuse.'],
    ['7. How information is used', 'Information is used to authenticate users, execute API requests, deliver email, verify domains, provide webhook events, enforce suppressions and limits, show operational activity, detect abuse, maintain security, and operate the service.'],
    ['8. External systems', 'Email necessarily interacts with DNS infrastructure, recipient mail servers, and mail delivery systems. Webhook events are delivered to endpoints selected by account administrators. Information sent to those systems is also subject to their own handling practices.'],
    ['9. Security choices', 'Keep API keys and webhook secrets in secure server-side storage, restrict scopes to what an integration needs, rotate credentials when exposure is suspected, and remove integrations or sender domains that are no longer required.'],
    ['10. Questions', 'For privacy questions or requests relating to a specific deployment, contact the administrator or support channel responsible for that CS Mailer service.']
  ]
}

export function LegalPage({ kind }: { kind: 'terms' | 'privacy' }) {
  const navigate = useNavigate()
  const isTerms = kind === 'terms'
  const title = isTerms ? 'Terms of Service' : 'Privacy Policy'
  const icon = isTerms ? <ShieldCheck size={22} /> : <LockKeyhole size={22} />
  return <main className="legal-page">
    <header className="legal-nav"><button className="brand-button" onClick={() => navigate('/')}><BrandLogo /></button><button className="button button--ghost" onClick={() => navigate('/')}><ArrowLeft size={15} />Back to CS Mailer</button></header>
    <section className="legal-hero"><div className="legal-hero__icon">{icon}</div><p className="public-kicker">CS Mailer · Developer email infrastructure</p><h1>{title}</h1><p>This page is written for the CS Mailer developer mail platform and the operational data it handles.</p><span>Last updated September 20, 2026</span></section>
    <div className="legal-layout"><aside><strong>Contents</strong>{sections[kind].map(([heading], index) => <a key={heading} href={`#legal-${index + 1}`}>{heading}</a>)}</aside><article className="legal-document"><div className="legal-context-grid"><span><Mail size={17} /><b>Transactional email</b></span><span><Server size={17} /><b>API infrastructure</b></span><span><Webhook size={17} /><b>Delivery events</b></span></div>{sections[kind].map(([heading, copy], index) => <section key={heading} id={`legal-${index + 1}`}><h2>{heading}</h2><p>{copy}</p></section>)}<div className="legal-end"><BrandLogo compact /><div><strong>CS Mailer</strong><p>Built for developer-controlled transactional email workflows.</p></div><ExternalLink size={16} /></div></article></div>
    <footer className="legal-footer"><span>© {new Date().getFullYear()} CrescentSphere · CS Mailer</span><button onClick={() => navigate(isTerms ? '/privacy' : '/terms')}>{isTerms ? 'Privacy Policy' : 'Terms of Service'}</button></footer>
  </main>
}

export function NotFoundPage({ signedIn }: { signedIn: boolean }) {
  const navigate = useNavigate()
  return <main className="not-found"><BrandLogo /><div className="not-found__code">404</div><h1>This route is not part of CS Mailer.</h1><p>Return to the developer mail platform or open your console.</p><div><button className="button button--secondary" onClick={() => navigate('/')}>Home</button><button className="button button--primary" onClick={() => navigate(signedIn ? '/overview' : '/login')}>{signedIn ? 'Open console' : 'Sign in'}</button></div></main>
}
