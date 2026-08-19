# CrescentSphere Mailer

Production-oriented React console for the CrescentSphere transactional developer email platform. Hosted mailbox features remain a separate future application and deployment.

## Local development

```bash
./manage install
./manage frontend-dev
```

Open `http://localhost:5173/`. Navigation uses hash routes so the current static deployment works without server-side route configuration.

## Verification

```bash
./manage frontend-build
```

## Docker

```bash
./manage dev
```

The Compose stack serves the compiled app through Nginx at
`http://localhost:8081` and exposes `GET /healthz` for container checks.

## Integration boundary

The current data is intentionally local UI fixture data. The next implementation layer should add a small typed API client around the Axum endpoints, then replace fixtures feature by feature. Authentication, API keys, sending, and billing must be enforced by the server; browser state is never an authorization boundary.

## Backend

The Rust/Axum backend lives in [`../backend/`](../backend/README.md). It owns the
API and worker service boundaries, PostgreSQL and NATS JetStream services,
validated configuration, request tracing, health endpoints, delivery, and
production deployment tooling.
