import { Link } from 'react-router-dom'
import { CopyButton } from './shared'

const smtpExample = `from email.message import EmailMessage
from getpass import getpass
import smtplib
import ssl

# Replace both addresses before running this on your own computer.
message = EmailMessage()
message["From"] = "no-reply@your-verified-domain.example"
message["To"] = "you@example.com"
message["Subject"] = "CS Mailer SMTP test"
message.set_content("SMTP submission is working.")

with smtplib.SMTP_SSL(
    "smtp.mailer.crescentsphere.com", 2465,
    context=ssl.create_default_context(), timeout=20,
) as smtp:
    smtp.login("mailer", getpass("Production API key: "))
    smtp.send_message(message)

print("Accepted for delivery. Check the Emails page for status.")`

export function SmtpGuide() {
  return <section className="docs-section" id="smtp">
    <p className="docs-number">SMTP</p>
    <div className="smtp-guide">
      <h3>Send through SMTP</h3>
      <p>Use your application&apos;s SMTP settings when it cannot call the HTTP API. The current public endpoint accepts authenticated mail on the ports below. This is CS Mailer submission, separate from mailbox SMTP at <code>smtp.crescentsphere.com</code>.</p>
      <ol className="smtp-steps">
        <li>Select <strong>Production</strong> in the console and verify your sending domain on the <Link to="/domains">Domains</Link> page. Wait for the domain to show as verified.</li>
        <li>Create a <strong>production</strong> key with <code>emails:send</code> scope on the <Link to="/api-keys">API keys</Link> page. Give each application its own key, save it when shown, and keep it on the server.</li>
        <li>Enter the settings below in your application. Use a From address on the verified domain. Choose the port and matching TLS mode together.</li>
        <li>Send one message to an inbox you control. Check the <Link to="/emails">Emails</Link> page for queued and delivery events.</li>
      </ol>
      <div className="smtp-settings" role="table" aria-label="CS Mailer SMTP settings">
        <div role="row"><strong role="rowheader">Server</strong><code role="cell">smtp.mailer.crescentsphere.com</code></div>
        <div role="row"><strong role="rowheader">SSL/TLS</strong><span role="cell"><code>2465</code> · implicit TLS</span></div>
        <div role="row"><strong role="rowheader">STARTTLS</strong><span role="cell"><code>2587</code> · STARTTLS required</span></div>
        <div role="row"><strong role="rowheader">Username</strong><code role="cell">mailer</code></div>
        <div role="row"><strong role="rowheader">Password</strong><span role="cell">Your production CS Mailer API key (<code>cs_live_…</code>)</span></div>
      </div>
      <p>Ports <code>465</code> and <code>587</code> on the current server belong to the mailbox service. Set the Mailer ports above explicitly; do not rely on your library&apos;s defaults. Test keys (<code>cs_test_…</code>) simulate delivery and do not send to recipients.</p>
      <div className="smtp-example">
        <div className="smtp-example__heading"><strong>Test with Python 3</strong><CopyButton value={smtpExample} label="Copy" /></div>
        <pre><code>{smtpExample}</code></pre>
      </div>
      <p>An SMTP <code>250 queued</code> response means Mailer accepted the message for processing. Check the Emails page or delivery webhooks for the final outcome. If authentication fails, verify the username, key environment, scope, and key status. If submission is rejected, check the verified From domain, recipient, message size, and account limits. A temporary <code>4xx</code> response should be retried later.</p>
    </div>
  </section>
}
