# Stalwart domain provisioning

This upgrade lets Mailer onboard and verify sending domains without Amazon SES.
Stalwart stores each domain and its private RSA DKIM key. Mailer stores only the
public DNS value and Stalwart object identifiers.

## One-time Stalwart setup

1. Start the independent stack with `sh manage stalwart-up`. It creates the
   private Docker network `crescentsphere-mail-transport`; the Mailer API joins
   that network only to reach Stalwart's management listener.
2. In Stalwart, create a dedicated service account for Mailer and issue an API
   key in **Replace** permission mode. Grant only:
   `sysDomainGet`, `sysDomainQuery`, `sysDomainCreate`, `sysDomainUpdate`,
   `sysDkimSignatureGet`, `sysDkimSignatureQuery`,
   `sysDkimSignatureCreate`, and `sysDkimSignatureDestroy`.
3. Keep the default DKIM signing policy. It signs authenticated submissions
   when the sender domain is a local enabled domain. Mailer registers the
   bounce subdomain as an alias so the aligned envelope sender is also local.
4. Create a non-admin SMTP submission principal for the worker. The management
   API key and SMTP password must be different credentials.

Set these values in Mailer's production `.env`:

```env
DOMAIN_PROVIDER=stalwart
STALWART_API_URL=http://stalwart:8080
STALWART_API_TOKEN=replace-with-the-restricted-api-key
MTA_PUBLIC_HOST=smtp.crescentsphere.com
MTA_PUBLIC_IPV4=152.53.178.165
MTA_RETURN_PATH_PREFIX=bounce
```

`STALWART_API_URL` may use cleartext HTTP only on the private Docker service
network. Keep the loopback admin port and this Docker network off the public
Internet. Mailer refuses a public cleartext management URL in production.

## User domain flow

Adding a domain creates or adopts the exact Stalwart Domain object, registers
`bounce.<domain>` as its alias, creates a unique 2048-bit RSA DKIM signature,
and returns these provider-neutral records:

- Mailer ownership TXT at `_mailer-verification.<domain>`.
- RSA DKIM TXT at `<selector>._domainkey.<domain>`.
- SPF TXT and MX at `bounce.<domain>` for the dedicated return path.
- DMARC TXT at `_dmarc.<domain>` as a recommended record.

The user may publish the records at any DNS provider. Cloudflare OAuth remains
an optional one-click shortcut and is not required for verification. Mailer
checks public DNS every 30 seconds and changes only the affected record states.

`POST /v1/domains/{id}/rotate-dkim` creates a new key without removing the old
signing key. Once the new TXT record is public, Mailer retires the old private
key in Stalwart and removes the old record from its displayed instructions.
The stale public TXT can be deleted later at providers that do not support the
Cloudflare shortcut; it cannot be used after the old private key is destroyed.

Disabling a Stalwart-backed domain first disables its Domain object, then marks
the Mailer domain disabled. SES-backed domains created before migration remain
identified as SES domains and are never deleted from SES by Mailer.

## Deployment order

Run these commands on the VPS after creating the restricted API key:

```bash
sh manage stalwart-up
sh manage preflight
sh manage deploy
```

Before customer traffic, add a fresh test subdomain, publish its records
manually, verify it in Mailer, rotate DKIM once, publish the replacement TXT,
verify again, and disable it. Then submit a message and confirm SPF, DKIM and
DMARC pass from at least three independent receivers.
