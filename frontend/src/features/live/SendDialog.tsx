import { FormEvent, useMemo, useState } from 'react'
import { Check, FileText, Paperclip, Send, X } from 'lucide-react'
import { api } from '../../lib/api/client'
import { Domain, Envelope, Environment, ErrorNotice, Field, Notice, queryClient, Submit, useAction, useDialogLifecycle, useResource } from './shared'

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

export function SendDialog({ environment, close, queued }: { environment: Environment; close: () => void; queued: (id: string) => void }) {
  const domains = useResource<Domain[]>('/v1/domains', 30_000), action = useAction()
  const [id, setId] = useState(''), [files, setFiles] = useState<File[]>([]), [subjectLength, setSubjectLength] = useState(0), [idempotencyKey] = useState(() => crypto.randomUUID())
  const safeClose = () => { if (!action.busy) close() }
  const dialogRef = useDialogLifecycle(safeClose)
  const totalBytes = useMemo(() => files.reduce((sum, file) => sum + file.size, 0), [files])

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const form = event.currentTarget, data = new FormData(form)
    await action.run(async () => {
      if (files.length > 10 || files.some(file => file.size > 10_000_000) || totalBytes > 20_000_000) throw new Error('Use at most 10 files, 10 MB each and 20 MB total.')
      const attachments = await Promise.all(files.map(file => new Promise<{ filename: string; content_type: string; content: string }>((resolve, reject) => {
        const reader = new FileReader(); reader.onload = () => resolve({ filename: file.name, content_type: file.type || 'application/octet-stream', content: String(reader.result).split(',')[1] }); reader.onerror = () => reject(new Error('Unable to read attachment')); reader.readAsDataURL(file)
      })))
      const split = (name: string) => String(data.get(name) ?? '').split(',').map(value => value.trim()).filter(Boolean)
      const to = split('to'), cc = split('cc'), bcc = split('bcc')
      const recipientCount = to.length + cc.length + bcc.length
      if (!to.length) throw new Error('Add at least one To recipient.')
      if (recipientCount > 50) throw new Error('To, CC and BCC can contain at most 50 recipients in total.')
      const body = { from: data.get('from'), to, cc, bcc, subject: data.get('subject'), text: data.get('text'), environment, ...(attachments.length ? { attachments } : {}) }
      const response = await api.post<Envelope<{ id: string }>>('/v1/emails', body, { headers: { 'Idempotency-Key': idempotencyKey } })
      setId(response.data.id)
      await queryClient.invalidate(['/v1/emails', '/v1/auth/session', '/v1/workspace'])
      queued(response.data.id)
    })
  }

  return <div className="modal-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) safeClose() }}><section ref={dialogRef} tabIndex={-1} className="modal send-modal" role="dialog" aria-modal="true" aria-labelledby="send-title" aria-busy={action.busy}><div className="modal__header"><div><p className="eyebrow">{environment === 'test' ? 'Simulated delivery' : 'Production delivery'}</p><h2 id="send-title">Send with CS Mailer</h2></div><button className="icon-button" aria-label="Close send form" onClick={safeClose} disabled={action.busy}><X size={18} /></button></div><div className="modal__body">{id ? <div className="send-success"><span><Check size={24} /></span><h3>Email accepted</h3><p>CS Mailer queued the message. The Emails page will continue tracking its delivery state automatically.</p><code>{id}</code><div className="send-success__actions"><button className="button button--primary" onClick={safeClose}>Done</button></div></div> : <form className="form-stack" onSubmit={submit}>
    <Notice tone={environment === 'test' ? 'info' : 'warning'}>{environment === 'test' ? 'Test mode simulates delivery and does not send mail to the recipient.' : 'Production mode sends real email through CS Mailer. Use a verified sender domain.'}</Notice>
    <Field label="From"><input name="from" type="email" required defaultValue={environment === 'test' ? 'sender@sandbox.mailer.invalid' : ''} list="sender-domains" placeholder="sender@mail.example.com" /><datalist id="sender-domains">{domains.result?.data.filter(domain => domain.status === 'verified').map(domain => <option key={domain.id} value={`sender@${domain.domain}`} />)}</datalist></Field>
    <Field label="To" hint="Comma-separated. To + CC + BCC can contain up to 50 recipients."><input name="to" type="text" inputMode="email" required placeholder="recipient@example.com" /></Field>
    <div className="form-two"><Field label="CC"><input name="cc" type="text" inputMode="email" placeholder="Optional" /></Field><Field label="BCC"><input name="bcc" type="text" inputMode="email" placeholder="Optional" /></Field></div>
    <Field label="Subject"><div className="input-counter"><input name="subject" required maxLength={998} onChange={event => setSubjectLength(event.target.value.length)} placeholder="A concise transactional subject" /><span>{subjectLength}/998</span></div></Field>
    <Field label="Plain-text message"><textarea name="text" rows={8} required placeholder="Write the transactional message body…" /></Field>
    <div className="attachment-field"><div><span className="field__label">Attachments</span><small>Up to 10 files · 10 MB each · 20 MB total</small></div><label className="attachment-picker"><Paperclip size={15} /><span>Add files</span><input name="attachments" type="file" multiple hidden onChange={event => setFiles(Array.from(event.target.files ?? []))} /></label></div>
    {!!files.length && <div className="attachment-list">{files.map((file, index) => <div key={`${file.name}-${file.lastModified}`}><FileText size={15} /><span><strong>{file.name}</strong><small>{formatBytes(file.size)}</small></span><button type="button" className="icon-button" aria-label={`Remove ${file.name}`} onClick={() => setFiles(current => current.filter((_, currentIndex) => currentIndex !== index))}><X size={14} /></button></div>)}<small className={totalBytes > 20_000_000 ? 'is-error' : ''}>{formatBytes(totalBytes)} total</small></div>}
    <p className="muted">If the request times out, retry the same submission with the same idempotency key rather than creating a duplicate application event.</p>
    <ErrorNotice error={action.error || domains.error} /><div className="form-actions"><button type="button" className="button button--ghost" disabled={action.busy} onClick={safeClose}>Cancel</button><Submit busy={action.busy}><Send size={14} />{environment === 'test' ? 'Queue simulation' : 'Queue production email'}</Submit></div>
  </form>}</div></section></div>
}
