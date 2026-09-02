# CrescentSphere Mailer

Production-oriented React console for the CrescentSphere transactional developer email platform. Hosted mailbox features remain a separate future application and deployment.

## Local development

```bash
sh manage install
sh manage frontend-dev
```

Open `http://localhost:5173/`. Navigation uses hash routes so the current static deployment works without server-side route configuration.

## Verification

```bash
sh manage frontend-build
```

## Docker

```bash
sh manage dev
```

The Compose stack serves the compiled app through Nginx at
`http://localhost:8081` and exposes `GET /healthz` for container checks.

## Integration boundary

The active console is in `src/features/live/` and uses the typed API client for sessions, verified public signup, password reset, domains, keys, sending, activity, webhooks, and suppressions. `App.tsx` mounts this console; older feature/fixture files remain unmounted design references. No billing, MFA, template, or team-management screens are exposed. Server permissions remain authoritative. Test/production is selected explicitly, and test sends never send recipient email.

## Backend

The Rust/Axum backend lives in [`../backend/`](../backend/README.md). It owns the
API and worker service boundaries, PostgreSQL and NATS JetStream services,
validated configuration, request tracing, health endpoints, delivery, and
production deployment tooling.
