import {
  FileCode2, Globe2, KeyRound, LayoutDashboard, Send, ShieldBan, Webhook,
  type LucideIcon
} from 'lucide-react'

export const routes = {
  overview: '/',
  emails: '/emails',
  domains: '/domains',
  templates: '/templates',
  webhooks: '/webhooks',
  'api-keys': '/api-keys',
  suppressions: '/suppressions',
  settings: '/settings',
  billing: '/billing'
} as const

export type AppRoute = keyof typeof routes

export const routeTitles: Record<AppRoute, string> = {
  overview: 'Overview',
  emails: 'Emails',
  domains: 'Domains',
  templates: 'Templates',
  webhooks: 'Webhooks',
  'api-keys': 'API keys',
  suppressions: 'Suppressions',
  settings: 'Settings',
  billing: 'Billing'
}

export const navGroups: { label: string; items: { route: AppRoute; label: string; icon: LucideIcon; count?: string }[] }[] = [
  {
    label: 'Workspace',
    items: [
      { route: 'overview', label: 'Overview', icon: LayoutDashboard },
      { route: 'emails', label: 'Emails', icon: Send, count: '24' },
      { route: 'domains', label: 'Domains', icon: Globe2 },
      { route: 'templates', label: 'Templates', icon: FileCode2 },
      { route: 'webhooks', label: 'Webhooks', icon: Webhook }
    ]
  },
  {
    label: 'Configuration',
    items: [
      { route: 'api-keys', label: 'API keys', icon: KeyRound },
      { route: 'suppressions', label: 'Suppressions', icon: ShieldBan }
    ]
  }
]

export function routeFromPath(pathname: string): AppRoute {
  if (pathname.startsWith('/emails/')) return 'emails'
  if (pathname.startsWith('/domains/')) return 'domains'
  if (pathname.startsWith('/webhooks/')) return 'webhooks'
  if (pathname.startsWith('/templates/')) return 'templates'
  const match = Object.entries(routes).find(([, path]) => path === pathname)
  return (match?.[0] as AppRoute | undefined) ?? 'overview'
}
