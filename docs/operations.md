# Operating Corqty

This guide covers the supplied single-host Compose deployment. PostgreSQL is
the durable database and queue; the API also runs cron, lease recovery, and
retention loops. Workers keep only attempt-local scratch files. The web service
is a production Next.js standalone build.

## HTTPS deployment

Use Docker Compose 2.24.4 or newer; the production override uses `!reset` to
remove local port mappings. Initialize credentials using `scripts/init-local.sh`
or `scripts/init-local.ps1`, then keep `.env` private. Set `CORQTY_DOMAIN` in that
file to your actual hostname, without a scheme or path. Point its DNS records
to the host and permit inbound TCP 80 and 443 for Caddy's HTTPS service and
certificate issuance. Do not publish database, API, or worker ports externally.

```sh
docker compose -f docker-compose.yml -f docker/compose.production.yml config --quiet
docker compose -f docker-compose.yml -f docker/compose.production.yml up --build --wait
```

The override starts Caddy with persistent certificate/configuration volumes,
publishes only 80/443, sets the public URL to `https://$CORQTY_DOMAIN`, enables
secure session cookies, and disables both trusted shell and runtime host
commands. Configuration validation does not prove public DNS, certificate
issuance, or reachability; verify those against your own deployment.

Caddy forwards to the web service, which proxies `/api/v1` and `/hooks/v1` to
the API. Preserve request bodies exactly for webhook signatures. Do not buffer
or compress SSE responses: they use `Cache-Control: no-cache, no-transform` and
must flush progressively. If using another ingress, forward these paths and
allow long-lived SSE connections and plugin uploads of approximately 34 MiB
(25 MiB components encoded as base64). The remaining request limits still apply.

`CORQTY_API_ORIGIN` is a web build argument. Rebuild the web image when changing
the internal API address; changing a runtime environment variable alone does
not rewrite an existing Next.js build.

Registration is open by default and creates a new organization. For a private
beta, restrict access at your ingress or identity-aware gateway. Invitation
links are copyable; SMTP delivery, OIDC, billing, and email account recovery are
outside this release. Set expectations and an account-support process before
accepting customers. No hosted-service SLA is supplied by this repository.

## Credentials and encryption keys

`CORQTY_MASTER_KEY` is base64 for 32 cryptographically random bytes. API and
workers must use the same key. PostgreSQL stores only ciphertext, per-version
nonces, and key-version metadata for secrets and webhook signing material.
Organization and object identity are authenticated during encryption.

Back up the master key in a protected secret store separately from database
backups, and verify that the recovery operator can retrieve both. Restrict host,
container, and backup access. Compose environment values are visible to people
with Docker administration privileges; Docker access is privileged access.

Do not rotate the master key by merely replacing `.env`. The current release
has key-version metadata but no key ring or online rotation tool. Rotation
requires a reviewed maintenance migration that uses both old and new keys,
re-encrypts every organization secret and webhook secret while preserving IDs,
and verifies decryption before services switch to the new key. Keep a recovery
copy of the old database/key pair until that migration is validated. Losing
the only matching key makes encrypted values unrecoverable.

The PostgreSQL initialization script creates roles only on a fresh database
volume. Editing password variables later does not change existing role
passwords. Coordinate database password changes with the corresponding
`DATABASE_URL_OWNER`, `DATABASE_URL_API`, and `DATABASE_URL_WORKER` settings.
An administrator can use PostgreSQL's interactive `\password` command to avoid
placing passwords in shell history. API and worker roles must remain
non-superuser and without `BYPASSRLS`; only the separate migration owner has the
rights required by security-definer functions.

## Backup and restore

Choose a backup schedule and recovery objective appropriate to your data.
The following examples use a POSIX shell, the default database name `corqty`,
and the production Compose files. Substitute the actual project/database names
when they differ. The commands copy archive files rather than piping binary
data through PowerShell's text redirection.

```sh
dc() { docker compose -f docker-compose.yml -f docker/compose.production.yml "$@"; }
backup_stamp=$(date -u +%Y%m%dT%H%M%SZ)
mkdir -p "backups/$backup_stamp"
dc exec -T postgres pg_dump -U postgres -d corqty --format=custom --file=/tmp/corqty.dump
dc exec -T postgres pg_dumpall -U postgres --globals-only --file=/tmp/corqty-globals.sql
dc cp postgres:/tmp/corqty.dump "backups/$backup_stamp/corqty.dump"
dc cp postgres:/tmp/corqty-globals.sql "backups/$backup_stamp/corqty-globals.sql"
dc exec -T postgres rm /tmp/corqty.dump /tmp/corqty-globals.sql
```

Encrypt and move these backups to access-controlled storage outside the
application host. Database dumps include password hashes, sessions, audit data,
and tenant data; global-role dumps can include role password hashes. Store the
exact release revision, PostgreSQL major version, and configuration with the
backup record, with secrets in your protected secret store. A custom-format
dump includes the application schema and migration history; global roles are
separate PostgreSQL objects. See PostgreSQL's
[SQL dump documentation](https://www.postgresql.org/docs/18/backup-dump.html).

Restore into an isolated environment first. Use the same PostgreSQL major
version and the original matching master key. Starting only `postgres` with
the original protected configuration on a new volume creates the expected role
names and credentials. Preserve the role names and privileges from
`docker/postgres/init/001-roles.sh`; reconcile custom roles from the globals
archive rather than replaying duplicate role creation into an initialized
cluster.

```sh
dc up -d --wait postgres
dc cp ./backups/CHOSEN_BACKUP/corqty.dump postgres:/tmp/corqty.dump
dc exec -T postgres createdb -U postgres -O corqty_owner -T template0 corqty_restore
dc exec -T postgres pg_restore -U postgres --exit-on-error -d corqty_restore /tmp/corqty.dump
dc exec -T postgres rm /tmp/corqty.dump
```

These commands create a new database; they do not overwrite an existing one.
Point all three database URLs at `corqty_restore` for the recovery environment
and preserve the corresponding credentials. Start the original release with
the original master key, inspect organizations/workflow versions/audit history,
and execute controlled checks of secret resolution and webhook authentication.
Keep the recovery environment isolated: enabled cron triggers and queued runs
can execute as soon as API/workers start. Disable them or restrict outbound
access before restoring production data into a test environment.

Perform a documented restore drill before relying on a backup. `pg_dump` gives
a consistent database snapshot but not continuous point-in-time recovery; use
your PostgreSQL platform's WAL archiving/PITR service if your recovery objective
requires it. Restore behavior is documented in
[pg_restore](https://www.postgresql.org/docs/18/app-pgrestore.html).

## Upgrades without deleting data

1. Record the running revision and configuration, take a database backup, and
   verify that its matching master key is available.
2. Test the new release and forward migrations against a restored copy. Review
   migration notes and plugin host-contract compatibility.
3. Restrict new traffic and disable triggers for a maintenance window when an
   upgrade needs downtime. Stop web/API and drain workers before changing an
   incompatible schema.
4. Build the new images, run the migrator once, then start services:

   ```sh
   dc build
   dc run --rm migrate
   dc up -d --wait
   ```

5. Check health, registration/login, a controlled run, live events, a webhook,
   and worker compatibility before restoring normal traffic. Re-enable only
   the intended triggers.

Migrations are forward-only, tracked by checksums, and run as `corqty_owner`.
Never edit a migration already applied to a deployed database. Do not use
`docker compose down -v` as an upgrade step: it deletes persistent data.
An older Milestone 1 installation may need its role ownership/grants reconciled
to the current initialization script before migrating; back it up and perform
that role migration explicitly instead of recreating its volume.

Rolling back an image does not roll back a schema. If compatibility is not
established, restore the previous database/configuration/key pair into a new
environment and switch traffic after verification. Retain the failed upgrade
environment for diagnosis instead of overwriting the only recovery copy.

## Workers, plugins, and resource limits

```sh
dc up -d --wait --scale worker=3
```

Worker containers have no exposed ports. Each has a read-only root filesystem,
a 256 MiB `/tmp` tmpfs, a 2 GiB memory cap, a 256-process cap, dropped Linux
capabilities, and `no-new-privileges`. These are deployment defaults, not a
guarantee that every allowed workload fits. Review host capacity and tune worker
concurrency with `CORQTY_WORKER_CAPACITY` (default 4). Each process uses database
connections for its running attempts plus claiming; account for this when
setting PostgreSQL connection limits.

Runtime components are limited to 25 MiB uploads, 256 MiB linear memory per
guest, 100 million fuel units per call, and the node deadline. Scratch files are
attempt-local. Network approvals are exact host/port permissions; an approved
destination may be an internal service. Restrict worker egress at your network
layer when needed. New plugin bytes or versions need fresh capability approval.
Disabling a plugin prevents new publishes/runs but deliberately lets existing
runs finish, including queued descendants.

The defaults in `.env.example` are hard upper bounds. Reduce component and memory
budgets with `CORQTY_MAX_COMPONENT_BYTES` and `CORQTY_MAX_WASM_MEMORY_BYTES`, and
use the documented graph/input/output/log/timeout settings for smaller workloads.
Keep API and worker settings aligned; invalid or above-ceiling values stop startup.

Keep shell disabled for untrusted tenants. With trusted execution enabled,
shell commands and approved runtime host commands execute as the worker user.
They may reach its database/master-key credentials and other organizations'
jobs. Process-group cancellation stops ordinary subprocess trees; a process
that deliberately creates a new session or daemonizes can escape that group.
Stop/recreate the worker container to clear such processes. Do not mount a
Docker socket, privileged devices, or host directories into workers.

The worker drains for 25 seconds on shutdown, then abandons unfinished attempts;
lease recovery can retry them. This is at-least-once execution, not exactly-once
external side effects. Plugin authors should deduplicate using the stable
execution ID and reconcile any side effects after a crash.

## Monitoring and incident recovery

Use `dc ps`, `dc logs --tail=200 api worker`, and the internal
`/api/v1/health` endpoint for service checks. A healthy process alone does not
prove jobs are progressing. Monitor run queue age, worker heartbeats/compatible
host contracts, failed/retrying nodes, database disk/connection pressure, and
container OOM/restart events. `waiting_for_compatible_worker` means no live
worker advertises the required exact executor or host contract.

Re-run dependency audits when rebuilding and after new advisories appear.
CI checks JavaScript dependencies and a guarded Rust audit; the documented
lockfile-only RSA exception applies solely while no workspace target compiles
that optional SQLx MySQL dependency. See [audit scope](security.md#dependency-audit-scope).

For stalled execution, restore compatible worker capacity and database
connectivity first. Lease recovery runs every five seconds; do not manually
mark an active node successful or bypass its fencing token. Use the supported
cancel/rerun actions after examining possible external side effects. A database
outage preserves committed jobs; a crash between an external action and
completion can require reconciliation before retrying.

For a suspected credential or plugin incident, restrict ingress/egress, revoke
service tokens, disable affected triggers/plugins, and cancel affected runs.
Disabling a plugin alone does not stop existing runs. Stop workers if immediate
containment is necessary. Preserve database/audit evidence and relevant
configuration without exporting plaintext secrets into tickets or logs.

Rate counters are shared in PostgreSQL. Peer-based authentication/external
limits use the actual socket peer and ignore forwarding headers. Behind Caddy
and Next.js, many clients share one upstream peer bucket; this is not a
trustworthy per-end-user limit. Add client-aware rate limits at your trusted
ingress and size the backend bucket for aggregate traffic. Token/webhook actor
limits remain separate. See [automation](automation.md) for defaults and knobs.

Run events/logs expire after 30 days; completed runs and node inputs/outputs
expire after 90 days. Retention is hourly and leader-elected. It preserves
workflow versions, plugin versions, and audit events, but does not implement
legal holds, per-tenant policies, or a long-term archive. Audit growth is
unbounded in this release. Backups have their own retention policy and can
contain data already expired from the live database.

Secret filtering catches exact bound values in persisted node input views,
logs, errors, and outputs; a detected output secret fails the node. It cannot
guarantee detection of encoded, transformed, partial, or generated values.
Keep credentials in secret bindings rather than literal configuration or
ordinary workflow input. Review the [security policy](security.md).
