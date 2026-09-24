# Stalwart domain provisioning

Mailer onboards and verifies sending domains through the independent Stalwart MTA.
Stalwart stores each domain and its private RSA DKIM key. Mailer stores only the
public DNS value and Stalwart object identifiers.

Stalwart is also used by CS Mail for business mailboxes. Mailer must never
adopt an existing Stalwart domain based on its name: it may belong to CS Mail
or be managed by an operator. New Mailer domains carry a Mailer ownership
description. A retry may reconcile only a domain with that exact description;
an unmarked existing domain fails closed. An older Mailer row without a
provider domain ID may need an operator ownership review and explicit
migration before it can be re-provisioned. Do not remove or edit the other
product's domain to clear the conflict.

For a previously verified Mailer domain with stored Stalwart domain and DKIM
IDs, the background verifier can restore a missing ownership description. It
requires the current public `_mailer-verification.<domain>` TXT challenge to
match the Mailer domain ID, the stored provider domain ID to resolve to the
same name, the stored DKIM signature to belong to that provider domain, and
the expected Mailer return-path alias and manual DKIM configuration. It only
updates a blank description. A different marker, missing proof, or mismatched
provider object remains blocked for operator review. After Mailer deploys, wait
for reconciliation and retry CS Mail domain provisioning. Do not rename or
delete either product's domain to force adoption.

## Sharing the CrescentSphere root domain

CS Mail owns `crescentsphere.com` in Stalwart for receiving mail and mailboxes.
One explicitly selected Mailer workspace may also verify it as a sending domain.
Set both of these in Mailer's production `.env` before adding the domain:

```env
STALWART_SHARED_DOMAIN=crescentsphere.com
STALWART_SHARED_WORKSPACE_ID=<the-Mailer-workspace-UUID>
```

The UUID is the `workspaces.id` value in Mailer's PostgreSQL database for the
workspace that will send CrescentSphere service mail. Adding the domain from
any other workspace is rejected. The Stalwart Domain must already exist and
be enabled. Mailer adds only `bounce.crescentsphere.com` as an alias and a
Mailer-specific DKIM signature. It does not take ownership of, disable, or
rename CS Mail's Domain object. Removing the domain in Mailer disables its
Mailer record only; CS Mail's mailboxes remain active.

After adding the root domain in Mailer, publish its displayed
`_mailer-verification.crescentsphere.com`, DKIM selector, and
`bounce.crescentsphere.com` MX/SPF records alongside the root MX/SPF/DMARC.
The root SPF must authorize the actual Stalwart sending IP. Keep only one
SPF TXT and one DMARC TXT at each name. Then verify the domain in Mailer and
create a separate send-only API key for each service (such as Notes), with
an approved From address such as `notes@crescentsphere.com`. The existing
`mailer.crescentsphere.com` domain may remain for Mailer's own system mail.

Do not remove Mailer's displayed DNS records when importing the base DNS zone:
they are created after the shared domain is added and have distinct names.

## Sharing any customer domain with CS Mail

One Stalwart Domain object can support both products. A CS Mail business can
verify and attach mailboxes to a Mailer-owned domain. Mailer keeps its provider
ownership marker; CS Mail keeps a separate shared binding and must not delete
or disable the provider object while mailboxes use it.

If a customer adds a domain to Mailer after CS Mail has created it, Mailer first
shows only `_mailer-verification.<domain>`. Publish that TXT record (manually or
through the Cloudflare DNS connection) and run Verify. Only after public DNS
matches does Mailer add its bounce alias and its own `csmailer-...` DKIM
signature. Then publish the additional DNS records Mailer displays and verify
again. Keep CS Mail's apex MX, SPF, and existing DKIM records; the Mailer
bounce records are on a separate hostname. A conflicting single-value record
requires review instead of automatic replacement.

Removing a sending domain from Mailer disables its Mailer record and API
authorization. It does not disable the Stalwart Domain object, because CS Mail
may host mailboxes there. Orphaned provider objects require an operator check
of both products before any manual cleanup. Do not delete the shared Stalwart
domain to resolve a claim conflict.

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
  - IF `eq_ignore_case(authenticated_as, 'mailer-submit') || eq_ignore_case(authenticated_as, 'mailer-submit@mailer.crescentsphere.com') || eq_ignore_case(authenticated_as, 'cs-mail-submit') || eq_ignore_case(authenticated_as, 'cs-mail-submit@svc.crescentsphere.com')`
  - THEN `false`
  - ELSE `true`
- **Allowed Mechanisms**:
  - IF `listener != 'smtp' && is_tls`
  - THEN `[plain, login]`
  - ELSE `false`
- **Maximum failures**: `3`.
- **Wait after failure**: `5s`.

The two service identities submit for their respective verified customer
domains. Every other SMTP principal must still match its sender. Never set
both the exception and ELSE to `false`, because that disables sender matching
for every authenticated account.

In **Settings > MTA > Session > MAIL FROM Stage > Sender is allowed**, put the
same Mailer identity condition first:

- IF `eq_ignore_case(authenticated_as, 'mailer-submit') || eq_ignore_case(authenticated_as, 'mailer-submit@mailer.crescentsphere.com')`
- THEN `starts_with(sender_domain, 'bounce.') && is_local_domain(sender_domain)`
- IF `eq_ignore_case(authenticated_as, 'cs-mail-submit') || eq_ignore_case(authenticated_as, 'cs-mail-submit@svc.crescentsphere.com')`
- THEN `is_local_address(sender) && !starts_with(sender_domain, 'bounce.')`
- ELSE `(is_empty(authenticated_as) && !key_exists('spam-block', sender_domain)) || (!is_empty(authenticated_as) && is_local_address(sender))`

This limits the Mailer worker to its bounce subdomains and CS Mail's relay to
existing mailbox addresses, while preserving unauthenticated server-to-server
delivery on port 25. Keep these rules synchronized with the CS Mail runbook.
Test that each service credential is rejected when it tries to send for the
other product's sender address before enabling customer traffic. The SMTP
server's sender-domain policy is a separate control from each application's
domain verification and must be checked after Stalwart upgrades.

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

Disabling a domain revokes its Mailer authorization without disabling the
shared Stalwart Domain object. An operator must review both products before
removing an orphaned provider domain.

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
