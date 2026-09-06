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
   `authenticate`, `sysDomainGet`, `sysDomainQuery`, `sysDomainCreate`, `sysDomainUpdate`,
   `sysDkimSignatureGet`, `sysDkimSignatureQuery`,
   `sysDkimSignatureCreate`, and `sysDkimSignatureDestroy`.
   The generated API-key secret is displayed only once. Store that complete
   value as `STALWART_API_TOKEN` in Mailer's `.env`.
3. Configure **Settings > MTA > Inbound > Sender Authentication > DKIM Signing**
   so authenticated bounce-domain envelopes are signed by their parent sending
   domain. Add these conditions in order (replace `bounce.` if
   `MTA_RETURN_PATH_PREFIX` is different):

   | IF | THEN |
   | --- | --- |
   | `starts_with(sender_domain, 'bounce.') && is_local_domain(sender_domain) && !is_empty(authenticated_as)` | `strip_prefix(sender_domain, 'bounce.')` |
   | `is_local_domain(sender_domain) && !is_empty(authenticated_as)` | `sender_domain` |

   Keep ELSE as `false`. This avoids a per-domain hard-coded rule and makes
   `mailer+<id>@bounce.<customer-domain>` use the DKIM signatures belonging to
   `<customer-domain>`. The rule is a required one-time server policy; domain
   provisioning continues to register each bounce hostname as a local alias.
4. Create a non-admin SMTP submission principal for the worker. The management
   API key and SMTP password must be different credentials.

## Production SMTP session policy

The examples below assume the public listener IDs are `smtp` for port 25 and
`submissions` for implicit TLS on port 465. Replace both occurrences of the
example identity with the exact value of Mailer's `SMTP_USERNAME`.

In **Settings > MTA > Session > AUTH Stage**, use:

- **Require Authentication**: one ELSE expression, `listener != 'smtp'`.
- **Must match sender**:
  - IF `eq_ignore_case(authenticated_as, 'mailer-submit') || eq_ignore_case(authenticated_as, 'mailer-submit@mailer.crescentsphere.com')`
  - THEN `false`
  - ELSE `true`
- **Allowed Mechanisms**:
  - IF `listener != 'smtp' && is_tls`
  - THEN `[plain, login]`
  - ELSE `false`
- **Maximum failures**: `3`.
- **Wait after failure**: `5s`.

The narrow **Must match sender** exception is necessary because one Mailer
service identity submits for many verified customer domains. Every other SMTP
principal must still match its sender. Never set both the exception and ELSE to
`false`, because that disables sender matching for every authenticated account.

In **Settings > MTA > Session > MAIL FROM Stage > Sender is allowed**, put the
same Mailer identity condition first:

- IF `eq_ignore_case(authenticated_as, 'mailer-submit') || eq_ignore_case(authenticated_as, 'mailer-submit@mailer.crescentsphere.com')`
- THEN `is_local_domain(sender_domain)`
- ELSE `!is_empty(authenticated_as) || !key_exists('spam-block', sender_domain)`

This limits the Mailer worker to domains provisioned locally in Stalwart while
preserving normal unauthenticated server-to-server delivery on port 25.

Use port 465 with implicit TLS for the Mailer worker. Do not offer `PLAIN` or
`LOGIN` on a connection without TLS, do not offer AUTH on port 25, and do not
publish the loopback administration port. The SMTP greeting, Mailer
`SMTP_HELO_NAME`, certificate SAN, forward A record, and reverse PTR should all
use `smtp.crescentsphere.com`.

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

For every acceptance test, inspect the received message's original headers. The
DKIM `d=` value must be the verified sending domain, never its `bounce.` alias.
