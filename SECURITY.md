# Security policy

Corqty's security-sensitive areas include tenant isolation, authentication,
worker lease fencing, secret storage/redaction, runtime capabilities, and trigger
authentication. Security fixes target the current maintained branch. No support
window, response-time guarantee, or independent security certification is implied.

## Reporting a vulnerability

Use this repository's **Security → Report a vulnerability** option when private
reporting is available. Include affected revisions, deployment conditions,
minimal reproduction steps, impact, and a proposed mitigation if known. Use
synthetic accounts and secrets; exclude production credentials and tenant data.

If private reporting is unavailable, open a minimal issue asking a maintainer
for a private reporting channel, without exploit details or sensitive material.
Do not publish a working exploit or other users' information while arranging
coordinated disclosure. Maintainers will discuss remediation and disclosure
timing with the reporter when they can respond.

## Deployment boundaries

- Shell and runtime host commands execute as the shared worker user. Enable
  `CORQTY_SHELL_EXECUTOR` only when all command authors are trusted; it is not a
  hostile-tenant sandbox. The production Compose configuration disables it.
- Runtime components have bounded memory/fuel/deadlines and explicit host
  capabilities. Approving network access grants reachability to the named
  destination, including internal services; review it accordingly.
- Secret redaction detects exact bound values. Encoded, transformed, generated,
  or partial values are outside that guarantee. Do not put credentials in plain
  workflow configuration or ordinary run inputs.
- Protect `CORQTY_MASTER_KEY` and database credentials with host/secret-store
  access controls. Back up the key separately and securely. Changing it without
  re-encrypting data breaks stored secrets and webhook authentication.
- Serve public deployments over HTTPS with secure cookies, private database/API
  networking, current dependencies, and an upstream rate limiter where needed.
  The API intentionally does not trust arbitrary forwarding headers.

See [operations](docs/operations.md) for recovery and deployment details. For an
active incident, restrict access, preserve relevant audit evidence, revoke
affected tokens, disable affected triggers/plugins, and follow your incident
response process. Avoid placing incident secrets in public issue discussions.

## Dependency audit scope

CI runs the JavaScript audit and `scripts/audit-rust.ps1`, using `cargo-audit`
0.22.2. The Rust script first checks the complete workspace dependency tree for
all targets, then audits with warnings denied. It allows one explicit advisory,
`RUSTSEC-2023-0071`, only while the affected `rsa` crate remains absent from the
compiled dependency tree. SQLx retains that optional MySQL dependency in the
lockfile even though Corqty enables PostgreSQL only. The script fails if `rsa`
becomes a compiled dependency; the exception must then be removed and the
exposure remediated. This is an unused-dependency exception, not a claim that
the affected RSA implementation is safe or fixed.

The checked lockfile includes updated Rustls, WebPKI, root certificates, and
Tokio dependencies addressing the advisories found during release preparation.
Audit results are snapshots of known advisories; keep dependencies and the
advisory database current after deployment.
