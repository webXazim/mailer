import { FormEvent, useState } from 'react'
import { X } from 'lucide-react'
import { api } from '../../lib/api/client'
import { Domain, Envelope, Environment, ErrorNotice, Field, Submit, useAction, useResource } from './shared'
export function SendDialog({ environment, close, queued }: { environment: Environment; close: () => void; queued: () => void }) {
  const domains = useResource<Domain[]>('/v1/domains'), action = useAction()
  const [id, setId] = useState(''), [idempotencyKey] = useState(() => crypto.randomUUID())
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const data = new FormData(event.currentTarget)
    await action.run(async () => {
      const files = data.getAll('attachments').filter((file): file is File => file instanceof File && file.size > 0)
      if (files.length > 10 || files.some(file => file.size > 10_000_000) || files.reduce((sum, file) => sum + file.size, 0) > 20_000_000) throw new Error('Use at most 10 files, 10 MB each and 20 MB total.')
      const attachments = await Promise.all(files.map(file => new Promise<{ filename: string; content_type: string; content: string }>((resolve, reject) => {
        const reader = new FileReader(); reader.onload = () => resolve({ filename: file.name, content_type: file.type || 'application/octet-stream', content: String(reader.result).split(',')[1] }); reader.onerror = () => reject(new Error('Unable to read attachment')); reader.readAsDataURL(file)
      })))
      const split = (name: string) => String(data.get(name) ?? '').split(',').map(v => v.trim()).filter(Boolean)
      const body = { from: data.get('from'), to: split('to'), cc: split('cc'), bcc: split('bcc'), subject: data.get('subject'), text: data.get('text'), environment, ...(attachments.length ? { attachments } : {}) }
      const response = await api.post<Envelope<{ id: string }>>('/v1/emails', body, { headers: { 'Idempotency-Key': idempotencyKey } })
      setId(response.data.id); queued()
    })
  }
  return <div className="modal-backdrop"><section className="modal live-send" role="dialog" aria-modal="true" aria-labelledby="send-title"><div className="modal__header"><h2 id="send-title">Send {environment} email</h2><button className="icon-button" aria-label="Close send form" onClick={close}><X size={16} /></button></div><div className="live-panel__body">{id ? <><p className="live-notice" role="status">Email queued. View its status in Emails.</p><code>{id}</code><button className="button button--primary" onClick={close}>Done</button></> : <form className="form-stack" onSubmit={submit}><p>{environment === 'test' ? 'Simulated delivery only. No email is sent to the recipient.' : 'This sends real email through SES. Use a verified sending domain.'}</p><Field label="From"><input name="from" type="email" required defaultValue={environment === 'test' ? 'sender@sandbox.mailer.invalid' : ''} list="sender-domains" placeholder="sender@mail.example.com" /><datalist id="sender-domains">{domains.result?.data.filter(d => d.status === 'verified').map(d => <option key={d.id} value={`sender@${d.domain}`} />)}</datalist></Field><Field label="To (comma-separated, 50 recipients total)"><input name="to" required placeholder="recipient@example.com" /></Field><div className="form-two"><Field label="CC"><input name="cc" /></Field><Field label="BCC"><input name="bcc" /></Field></div><Field label="Subject"><input name="subject" required maxLength={998} /></Field><Field label="Plain-text message"><textarea name="text" rows={5} required /></Field><Field label="Attachments (optional)"><input name="attachments" type="file" multiple /></Field><p className="muted">After a timeout, retry this unchanged form. Its idempotency key prevents duplicate acceptance.</p><ErrorNotice error={action.error || domains.error} /><Submit busy={action.busy}>{environment === 'test' ? 'Queue simulation' : 'Queue real email'}</Submit></form>}</div></section></div>
}
