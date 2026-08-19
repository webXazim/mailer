export interface TemplateSummary { id: string; name: string; description: string; status: 'Published' | 'Draft'; updated: string; html: string; variables: string[] }
export const templateFixtures: TemplateSummary[] = [
  { id: 'welcome', name: 'Welcome email', description: 'Workspace onboarding message', status: 'Published', updated: '2 days ago', html: '<table role="presentation" width="100%"><tr><td style="padding:40px 48px"><h1>Welcome to {{product_name}}</h1><p>Hi {{first_name}}, your workspace is ready.</p><a href="{{action_url}}">Open your workspace</a></td></tr></table>', variables: ['first_name', 'product_name', 'action_url'] },
  { id: 'receipt', name: 'Payment receipt', description: 'Successful payment confirmation', status: 'Published', updated: 'Aug 11, 2026', html: '<h1>Payment received</h1><p>Thanks for your payment, {{first_name}}.</p>', variables: ['first_name', 'amount'] },
  { id: 'password-reset', name: 'Password reset', description: 'Secure password reset message', status: 'Draft', updated: 'Aug 04, 2026', html: '<h1>Reset your password</h1><p>Use the link below to choose a new password.</p>', variables: ['first_name', 'reset_url'] }
]

