export type DomainStatus = 'Verified' | 'Pending verification'

export interface DomainSummary {
  domain: string
  added: string
  status: DomainStatus
  identities: number
  messages: string
}

export const domainFixtures: DomainSummary[] = [
  { domain: 'mail.acme.dev', added: 'Aug 12, 2026', status: 'Verified', identities: 3, messages: '126,840' },
  { domain: 'notify.acme.dev', added: 'Aug 08, 2026', status: 'Verified', identities: 1, messages: '57,450' },
  { domain: 'acme.cloud', added: 'Aug 18, 2026', status: 'Pending verification', identities: 0, messages: '0' }
]

