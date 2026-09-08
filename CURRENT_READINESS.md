# Current readiness

Mailer has one production transport: the self-hosted Stalwart SMTP service.
Sending-domain provisioning, DKIM signing, delivery events, suppression updates,
and operational controls all use that path.

Production still requires live operational validation before customer traffic:

- Confirm the SMTP host A record and the server IP PTR resolve to each other.
- Confirm outbound TCP port 25 is available from the server.
- Publish and verify every DNS record shown by the Domains page.
- Send to several independent mailbox providers and confirm SPF, DKIM, and DMARC pass.
- Confirm delivered, temporary-failure, permanent-bounce, and complaint events arrive.
- Configure encrypted offsite backups and rehearse recovery.

The automated suite validates compilation, configuration, local event processing,
and deployment sequencing. It cannot prove external DNS, IP reputation, reverse
DNS, recipient acceptance, or production network reachability.
