# CS Mailer frontend

Production React/Vite interface for **CS Mailer**, the CrescentSphere developer email platform. The Rust/API contract is intentionally unchanged by this frontend upgrade.

## Included frontend

- Developer-mail public homepage, Terms of Service, Privacy Policy, and branded 404/error states.
- Authentication, verification, password recovery, and Cloudflare Turnstile integration using the existing API.
- Responsive console for overview, email activity/details, sender domains/DNS, API keys, webhooks, suppressions, and developer guidance.
- The supplied transparent CrescentSphere logo is the primary CS Mailer mark and is also used to derive transparent application icons.
- Shared resource cache with targeted invalidation, adaptive/background polling, stale-request cancellation, focus/reconnect refresh, and optimistic mutations where safe. Normal console actions update only affected React data; they do not reload the browser document.
- Offline awareness, request timeouts, accessible drawers/dialogs, keyboard focus handling, reduced-motion support, and CSP-compatible system font stacks.

## Development

```bash
npm ci
npm run verify:source
npm run dev
```

The API base defaults to `/api`. Override it with `VITE_API_URL` when required.

## Production verification

```bash
npm ci
npm run verify:source
npm run typecheck
npm run build
```

`verify:source` checks pinned dependencies/lockfile consistency, required brand assets, and guards against accidental browser-document reload patterns. The Docker build runs this verification before the Vite production build.

For the production compose file, the frontend is built with `nginx.production.conf`, which serves the SPA on the shared API network namespace and proxies `/api/` to the existing Rust service.
