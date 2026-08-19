export interface WebhookEndpoint { id: string; name: string; url: string; environment: 'Production' | 'Test'; events: string[]; status: 'Healthy' | 'Degraded'; lastSuccess: string }
export const webhookFixtures: WebhookEndpoint[] = [
  { id: 'wh_prod', name: 'Production events', url: 'https://api.acme.dev/webhooks/signal', environment: 'Production', events: ['email.sent', 'email.delivered', 'email.bounced', 'email.complained', 'email.delivery_delayed'], status: 'Healthy', lastSuccess: '1 min ago' },
  { id: 'wh_stage', name: 'Staging events', url: 'https://staging.acme.dev/hooks/mail', environment: 'Test', events: ['email.sent', 'email.delivered'], status: 'Healthy', lastSuccess: '18 min ago' },
  { id: 'wh_analytics', name: 'Analytics pipeline', url: 'https://events.acme.dev/email', environment: 'Production', events: ['All events'], status: 'Degraded', lastSuccess: '3 failed attempts' }
]

