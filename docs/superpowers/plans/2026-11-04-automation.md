# Milestone 5: automation and hardening

Implementation date: 2026-11-04. This plan refines the approved specification's
automation interfaces and records the delivered behavior. It is a maintained
implementation checklist rather than a literal replay of individual commits.

## Design contracts

1. Service tokens have random 256-bit bearer material, only SHA-256 hashes in
   PostgreSQL, one fixed `workflow:run` scope, and admin-only management.
2. Non-user runs record a nullable user actor plus a service-token or trigger
   identifier. Tenant context grants only read/run rights for currently enabled
   actors; editor/admin policies remain membership-based.
3. REST idempotency is transactional: an advisory lock on organization/route/
   actor/key serializes new rows, request hashes reject changed requests, and
   run creation plus response identity commit atomically. TTL is 24 hours.
4. Trigger versions are explicit and immutable until an authorized PUT. A
   webhook kind cannot change, and signing material is shown once and encrypted
   using the shared per-organization AES-GCM implementation with trigger-ID AAD.
5. Webhook signatures bind Unix seconds, a period, and raw request bytes.
   Verification uses HMAC's constant-time comparison. Five-minute clock tolerance,
   a 1 MiB body cap, and required idempotency identifiers apply before dispatch.
6. Cron parses standard five-field schedules in IANA zones. A database leader
   lock plus row locks covers each dispatch pass. Unique occurrence keys cover
   actual starts and skips; old misfires are summarized with a safe audit range.
   A failure rolls back only that run via a savepoint and records a rejection.
7. PostgreSQL rate counters span API replicas. Rate-limited requests receive
   Problem Details 429. Proxy IP headers are never trusted implicitly.
8. Retention has its own leader lock and removes events at 30 days and completed
   runs at 90 days. Version and audit tables are never cleanup targets.
9. Configurable limits are bounded by the existing hard ceilings. Invalid values
   fail API/worker startup. New node runs snapshot log and timeout caps; SQL is
   authoritative for persisted log truncation.

## Work and verification

- [x] Add forward-only automation and configurable-limit migrations with forced
  RLS and explicit function grants; no table rights granted to workers.
- [x] Add service-token APIs and a narrowly scoped bearer middleware branch.
- [x] Route manual, REST, cron, and webhook execution through one run constructor.
- [x] Add REST/webhook idempotency and actor-safe run audit metadata.
- [x] Add cron/webhook trigger management and shared input schema validation.
- [x] Add cron dispatch, retention, and replica-wide rate-limiting loops/helpers.
- [x] Add bounded domain limits, graph/runtime enforcement, and node log caps.
- [x] Add unit tests for timezone/cron/DST, HMAC raw-byte binding, and limit parsing.
- [x] Add HTTP/SQL integration tests for token secrecy/scope/revocation, replay
  conflicts, cross-tenant access, cron concurrency/overlap/misfires, webhook
  signatures/disable, role enforcement, retention, and rate counters.
- [x] Add a standalone automation smoke script and operator/API documentation.
- [ ] Run final integrated workspace checks and live Compose smoke after all
  parallel milestone changes are assembled; record results in the release audit.

Initial verification: six automation HTTP integration tests passed against
PostgreSQL; all API unit tests and the bounded-limits unit test passed. A review
found that restarting a cron iterator lost its second DST-fold occurrence;
bounded UTC-minute scanning fixes that behavior, with regressions for both fold
instants, restart during the repeated hour, and strict forward progress.
The final release audit, rather than this intermediate result, is authoritative
for assembled workspace and Compose verification.
