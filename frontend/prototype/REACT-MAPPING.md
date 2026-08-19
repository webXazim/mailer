# Signal Mail React Mapping

## Application Shell

- `AppShell`: fixed sidebar, sticky top bar, responsive content region
- `Sidebar`: workspace selector, primary navigation, usage summary, profile
- `Topbar`: breadcrumbs, search, notifications, account menu
- `PageHeader`: eyebrow, title, description, primary and secondary actions

## Shared Components

- `Button`, `IconButton`, `StatusBadge`, `Tag`, `ProgressBar`
- `Panel`, `MetricCard`, `DataTable`, `FilterBar`, `Pagination`
- `Dialog`, `Field`, `SelectField`, `PermissionField`
- `SearchField`, `SegmentedControl`, `Timeline`, `CodeBlock`
- `EmptyState`, `ErrorState`, `LoadingSkeleton`

## Feature Components

- Emails: `EmailTable`, `EmailStatus`, `EmailPreview`, `DeliveryTimeline`
- Domains: `DomainRow`, `DnsRecordTable`, `DomainHealthStrip`
- API keys: `ApiKeyTable`, `PermissionSelector`, `SecretRevealDialog`
- Webhooks: `EndpointRow`, `AttemptTable`, `PayloadViewer`, `SigningSecret`
- Templates: `TemplateCard`, `TemplateEditor`, `VariableList`
- Suppressions: `SuppressionTable`, `ReasonBadge`, `SuppressionDialog`
- Settings: `SettingsNav`, `TeamTable`, `BillingUsage`, `SecuritySetting`, `AuditLog`

## State Contract

Every data surface should support `loading`, `empty`, `ready`, and `error` states. Commands should support `idle`, `submitting`, `success`, and `failure`. Destructive actions require confirmation. Secrets must only be shown once after creation.

## Suggested React Structure

```text
src/
  app/
  components/
  features/
    emails/
    domains/
    api-keys/
    webhooks/
    templates/
    suppressions/
    settings/
  styles/
    tokens.css
    base.css
    components.css
```

The static class names can initially be retained during conversion, then replaced with component-scoped styles only where ownership becomes clearer.
