# CrescentSphere Mailer

Standalone transactional email platform for developers. Hosted mailbox
services are intentionally outside this repository.

## Projects

- [`frontend/`](frontend/README.md): React, Vite, and TypeScript console.
- [`backend/`](backend/README.md): Rust/Axum API, worker, PostgreSQL, NATS
  JetStream, SES, and R2 integration.

Run all normal development, verification, Docker, and deployment commands from
this directory through `./manage`. The `Makefile` provides optional aliases.

## Development

```bash
cp .env.example .env
./manage install
./manage frontend-dev
```

Start the complete Docker development stack:

```bash
./manage dev
```

Useful commands:

```bash
./manage help
./manage check
./manage logs
./manage down
./manage compose-config
```

Production deployment and credential instructions live in
[`backend/README.md`](backend/README.md). Production commands such as
`./manage preflight`, `./manage production-pull`, and `./manage production-up` also run
from this directory.

The complete Docker stack serves the console at `http://localhost:8081` and
the API at `http://localhost:8080`. When using the Vite server on port `5173`,
set `CONSOLE_ORIGIN=http://localhost:5173` before starting the backend.
