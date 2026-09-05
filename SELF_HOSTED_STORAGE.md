# Self-hosted object storage

Mailer can use Garage as its S3-compatible message-content store, removing the R2
or AWS object-storage runtime dependency. Garage runs as a separate Compose project
so application deploys do not restart storage. The S3 listener is available to the
application on the private transport network and is bound only to VPS loopback on
the host.

Initialize and start it after Stalwart has created the private transport network:

```bash
sh manage storage-init
vi .env.storage
sh manage storage-preflight
sh manage storage-up
sh manage storage-bootstrap
cat .storage/mailer.env
```

Copy the six generated `OBJECT_STORAGE_*` values into the application `.env`, run
`sh manage preflight`, and deploy. The generated Garage configuration and credentials
are mode 600 and ignored by Git. `storage-bootstrap` can adopt a bucket left by a
partial first run, but refuses when its application key already exists so it cannot
silently rotate a live credential. If parsing key creation ever fails, the raw CLI
result remains in the mode-600 `.storage/key-create.output` recovery file.

The application key receives only read and write access to its bucket. It is not a
Garage administrator and cannot create buckets. Keep the Garage RPC and admin
secrets separate from application credentials.

This initial topology uses one node with replication factor one and SQLite metadata.
It provides service independence, not hardware high availability. Configure an
offsite copy before relying on it. Set `BACKUP_OBJECT_STORAGE=true` and
`OBJECT_STORAGE_BACKUP_ENDPOINT=http://127.0.0.1:3900` in the application `.env`;
the existing backup timer will copy immutable object keys to
`$BACKUP_RCLONE_REMOTE/object-storage`. PostgreSQL backups retain the authoritative
object references, and a fresh Garage cluster can be bootstrapped with new credentials
before copying those objects back. Keep Garage's automatic metadata snapshots as an
additional local recovery aid. For higher availability, deploy at least three Garage nodes in separate failure
domains and follow Garage's layout and upgrade documentation; do not pretend that
three containers on one VPS are three independent replicas.

Normal lifecycle commands preserve the named data volumes:

```bash
sh manage storage-status
sh manage storage-logs
sh manage storage-restart
sh manage storage-down
```

Never add `-v` to the down command. Before upgrading the pinned image, read the
release migration notes, take and verify an offsite backup, pause sending, and test
object put/get/delete plus a real attachment after restart.
