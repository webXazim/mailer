export type KeyStatus = 'Active' | 'Paused'

export interface ApiKeySummary {
  id: string
  name: string
  description: string
  prefix: string
  permission: string
  environment: 'Production' | 'Test'
  createdBy: string
  lastUsed: string
  status: KeyStatus
}

export const apiKeyFixtures: ApiKeySummary[] = [
  { id: 'key_prod', name: 'Production API', description: 'Primary application key', prefix: 'sk_live_........7q2n', permission: 'Full access', environment: 'Production', createdBy: 'Alex Morgan', lastUsed: '2 min ago', status: 'Active' },
  { id: 'key_local', name: 'Local development', description: 'For local testing only', prefix: 'sk_test_........m8k4', permission: 'Send only', environment: 'Test', createdBy: 'Alex Morgan', lastUsed: 'Yesterday', status: 'Active' },
  { id: 'key_analytics', name: 'Analytics worker', description: 'Read delivery events', prefix: 'sk_live_........p1rx', permission: 'Read only', environment: 'Production', createdBy: 'Priya Shah', lastUsed: 'Aug 12, 2026', status: 'Paused' }
]

