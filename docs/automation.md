# Automation API

All organization routes use `/api/v1/orgs/{organizationId}`. Browser sessions need
the CSRF cookie/header on mutations. Admins manage service tokens; admins and
editors manage triggers. Members can read trigger metadata. Secret values and
webhook signing secrets never appear in later reads.

## Service tokens and REST runs

Create a token with `POST /service-tokens {"name":"Deploy client"}`. Copy the
`token` field from the creation response once; only its SHA-256 hash is stored.
Tokens have exactly the `workflow:run` scope. They authorize only
`POST /workflows/{workflowId}/runs`, using `Authorization: Bearer <token>`;
they cannot read resources, change workflows, or administer the organization.
`DELETE /service-tokens/{tokenId}` revokes a token immediately.

```sh
curl "$CORQTY_URL/api/v1/orgs/$ORG/workflows/$WORKFLOW/runs" \
  -H "Authorization: Bearer $CORQTY_TOKEN" \
  -H 'Content-Type: application/json' \
  -H 'Idempotency-Key: release-2026-11-04' \
  --data '{"version":1,"input":{"region":"eu-west-1"}}'
```

`version` pins an exact published version; omission selects the current published
version for a new request. An idempotency key is optional for this endpoint and
strongly recommended for retries. The first request returns 201, an identical
retry returns 200 with the original run, and a changed request returns 409
`idempotency_conflict`. Keys are isolated by organization, route, actor, and key
for 24 hours. Keys contain 1–200 printable ASCII characters without spaces.

## Cron triggers

```json
{
  "name": "Hourly import",
  "workflowId": "<workflow UUID>",
  "version": 1,
  "kind": "cron",
  "cron": "0 * * * *",
  "timezone": "Asia/Bangkok",
  "input": {},
  "enabled": true,
  "overlapPolicy": "skip"
}
```

Send this to `POST /triggers`. `PUT /triggers/{triggerId}` replaces the settings;
`DELETE` removes the trigger. A trigger always pins its explicit published
version. Publishing a new version never retargets a trigger. The kind cannot
change; create a new trigger when switching cron and webhook.

Cron uses five fields (minute, hour, day of month, month, day of week), an IANA
timezone, numeric or named weekdays, and Sunday 0 or 7. When both day fields
are restricted without a leading wildcard, either field can match. A wildcard
step such as `*/1` retains the other day's restriction, matching traditional
[cron day-field semantics](https://github.com/cronie-crond/cronie/blob/master/src/cron.c).
Nonexistent local times during a DST
spring transition are skipped; both UTC instants of a repeated local time during
fall-back are scheduled.
The dispatcher checks every 15 seconds and
elects one leader in PostgreSQL. Each scheduled UTC occurrence is unique.

`skip` is the default overlap policy: an unfinished run from that trigger skips
the next occurrence and records an audit event. `allow` permits overlap. A
five-minute grace window catches recent missed occurrences. Older misses are
skipped and summarized in an audit event with the skipped time range. Dispatch
failures such as a disabled executor are audited and do not spin indefinitely.

## Signed webhooks

Create a trigger with `kind: "webhook"`, no `cron` or `timezone`, and the desired
workflow/version. Creation returns `webhookPath` and `webhookSecret` once.
Store the secret at the sender. Corqty stores it encrypted with the master key.
To rotate it, create a replacement webhook, switch the sender, and delete the
old trigger. The request's raw JSON object is the workflow input; it is not
merged with the trigger's stored input (that input is used for cron).

The webhook endpoint is `/hooks/v1/{triggerId}` outside the browser session API.
Send:

- `X-Corqty-Timestamp`: current Unix time in whole seconds.
- `X-Corqty-Signature`: hex HMAC-SHA256 over `timestamp + "." + rawBody`, using
  the UTF-8 bytes of the signing secret as the key. An optional `sha256=` prefix
  is accepted. Sign the exact bytes sent, including whitespace.
- `Idempotency-Key`: a unique event identifier, required and retained 24 hours.
- A JSON workflow-input object of at most 1 MiB.

Timestamps more than five minutes in the past or future are rejected. Signature
comparison is constant-time. Invalid signatures return 401, missing/disabled
hooks return 404, changed reuse of an event identifier returns 409. Every source
uses the same workflow-input schema validation. Never put a signing secret in a
URL or query string.

## Limits, retention, and operations

Run events and logs expire after 30 days. Completed runs and their node inputs,
outputs, and summaries expire after 90 days. Retention runs hourly under a
database advisory lock. It preserves workflow/plugin versions and all audit
events. A live run is never removed by retention.

API replicas share fixed-window rate counters in PostgreSQL. Defaults are 30
authentication attempts per minute per socket peer, 30 invitation attempts,
300 external requests per peer, and 120 valid token/webhook requests per actor.
Adjust `CORQTY_AUTH_RATE_PER_MINUTE`, `CORQTY_TOKEN_RATE_PER_MINUTE`, and
`CORQTY_WEBHOOK_RATE_PER_MINUTE` within their documented limits. The API does not
trust caller-supplied forwarding headers; reverse proxies should enforce their
own per-client limits as well.

API and workers validate these positive integer settings at startup. Existing
hard ceilings cannot be raised in this release; lower values are supported:

| Variable | Default and hard ceiling |
|---|---:|
| `CORQTY_MAX_NODES` | 500 |
| `CORQTY_MAX_EDGES` | 1,000 |
| `CORQTY_MAX_RUN_INPUT_BYTES` | 1,048,576 |
| `CORQTY_MAX_NODE_INPUT_BYTES` | 1,048,576 |
| `CORQTY_MAX_NODE_OUTPUT_BYTES` | 1,048,576 |
| `CORQTY_MAX_WEBHOOK_BYTES` | 1,048,576 |
| `CORQTY_MAX_LOG_BYTES` | 10,485,760 |
| `CORQTY_MAX_TIMEOUT_SECONDS` | 14,400 |

`CORQTY_DEFAULT_TIMEOUT_SECONDS` defaults to 900, or a lower configured maximum.
Omitted timeout fields use that value. Node log and timeout ceilings are captured
when a run is created. Input/output limits are enforced by workers. Keep API and
worker settings aligned. The database truncates logs at a UTF-8 boundary and
emits exactly one truncation event.

Run `./scripts/smoke-automation.ps1` against a disposable local stack to exercise
token creation/revocation, REST deduplication, signed webhook deduplication,
secret metadata, and real cron dispatch (up to 75 seconds).
