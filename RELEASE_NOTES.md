# Release notes

## Independent SMTP release

Mailer now uses only the self-hosted Stalwart SMTP transport for production mail.
The worker submits authenticated SMTP, the API provisions Stalwart domains and
DKIM signatures, and signed Stalwart events drive delivery status, suppressions,
and customer webhooks.

Domain onboarding returns only records needed by the independent mail path:

- ownership verification TXT;
- DKIM TXT for the sending domain;
- MX and SPF for the return-path subdomain, with SPF restricted to the configured
  public IPv4 address;
- a recommended DMARC TXT record.

The SMTP hostname A record and the public IP PTR are server-wide infrastructure
records and must be configured once with the DNS host and VPS provider.

Existing installations should run `sh manage production-env-upgrade` before
deployment. Database migrations run automatically when the API starts and move
queued mail and domain provisioning to the independent transport.

Before real traffic, follow [Current readiness](CURRENT_READINESS.md) and run a
live delivery/event test against several independent recipients.
