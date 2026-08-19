export const endpoints = {
  session: '/v1/auth/session',
  login: '/v1/auth/login',
  signup: '/v1/auth/signup',
  logout: '/v1/auth/logout',
  workspace: '/v1/workspace',
  emails: '/v1/emails',
  domains: '/v1/domains',
  apiKeys: '/v1/api-keys',
  webhooks: '/v1/webhooks',
  templates: '/v1/templates',
  suppressions: '/v1/suppressions',
  billing: '/v1/billing'
} as const

