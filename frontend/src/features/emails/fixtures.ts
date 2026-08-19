export type EmailStatus = 'Delivered' | 'Processing' | 'Bounced'

export interface EmailSummary {
  subject: string
  recipient: string
  domain: string
  status: EmailStatus
  sentAt: string
  id: string
}

export const emailFixtures: EmailSummary[] = [
  { subject: 'Welcome to Acme Cloud', recipient: 'jordan@northstar.io', domain: 'mail.acme.dev', status: 'Delivered', sentAt: '2 min ago', id: 'msg_01H9K8D4QW' },
  { subject: 'Payment received', recipient: 'finance@orionlabs.co', domain: 'mail.acme.dev', status: 'Delivered', sentAt: '8 min ago', id: 'msg_01H9K8BW2F' },
  { subject: 'Reset your password', recipient: 'samira@atlas.design', domain: 'notify.acme.dev', status: 'Processing', sentAt: '14 min ago', id: 'msg_01H9K7YPQ9' },
  { subject: 'Your weekly report', recipient: 'team@northstar.io', domain: 'mail.acme.dev', status: 'Bounced', sentAt: '27 min ago', id: 'msg_01H9K7V1MX' },
  { subject: 'Invitation to Acme Cloud', recipient: 'lee@cinder.app', domain: 'mail.acme.dev', status: 'Delivered', sentAt: '42 min ago', id: 'msg_01H9K7M2ZA' }
]

