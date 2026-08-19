export type SuppressionReason = 'Bounced' | 'Complained' | 'Unsubscribed' | 'Manual'
export interface Suppression { id: string; email: string; domain: string; reason: SuppressionReason; detail: string; source: string; added: string }
export const suppressionFixtures: Suppression[] = [
  { id: 'sup_1', email: 'jordan@northstar.io', domain: 'mail.acme.dev', reason: 'Bounced', detail: 'Hard bounce', source: 'SES feedback', added: '2 min ago' },
  { id: 'sup_2', email: 'finance@orionlabs.co', domain: 'mail.acme.dev', reason: 'Complained', detail: 'Complaint', source: 'Recipient feedback', added: 'Aug 17, 2026' },
  { id: 'sup_3', email: 'samira@atlas.design', domain: 'notify.acme.dev', reason: 'Unsubscribed', detail: 'Unsubscribed', source: 'Preference center', added: 'Aug 15, 2026' },
  { id: 'sup_4', email: 'team@northstar.io', domain: 'mail.acme.dev', reason: 'Bounced', detail: 'Hard bounce', source: 'SES feedback', added: 'Aug 14, 2026' }
]

