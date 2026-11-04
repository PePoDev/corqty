# Documentation image inventory

The documentation uses actual product captures where available and conspicuous
SVG placeholders elsewhere. Placeholders are not screenshots, wireframes,
prototypes, or evidence of a completed UI check. The detailed capture briefs
live in the [visual tour](../visual-tour.md).

## Actual screenshots

Both JPEGs were captured on **2026-11-04** from `http://localhost:3000` running
the production Compose build. They use a synthetic `Web launch audit`
organization and `.test` email address. They were inspected locally, contain
no secret/service-token value, and were copied into this directory unchanged.
The webhook identifier in the trigger image belongs to a disabled test trigger;
its signing secret is not shown.

| Asset | Captured state | Review |
| --- | --- | --- |
| [`screenshots/runtime-success.jpg`](screenshots/runtime-success.jpg) | Published sample WASM workflow succeeded; selected node shows `hello from wasm` output | Real capture, safe synthetic data |
| [`screenshots/triggers.jpg`](screenshots/triggers.jpg) | Create form, pinned disabled cron/webhook records, authentication help expanded | Real full-page capture; one-time secret dismissed |

Integrity hashes of the unchanged captured files:

```text
runtime-success.jpg  d06c45a00cba1d3ec6dc80259edd49af9cf5b62cdf7b9acaa6fa1f591c66a30c
triggers.jpg         75b43d8f7c82740b1e52118234aae62262a5c662ab0e1dbf369d8456f68de2c6
```

These hashes document this capture provenance; update them if an actual image
is intentionally replaced. They are not a runtime/application security check.

## Pending real captures

The target filenames below are written as code, not links, because the files
do not yet exist. Keep links pointed at the existing SVG until its real
replacement has been captured and reviewed.

| Existing placeholder | Capture brief | Future file under `screenshots/` | Status |
| --- | --- | --- | --- |
| [`overview.svg`](placeholders/overview.svg) | [Workspace overview](../visual-tour.md#workspace-overview) | `overview.jpg` | Placeholder |
| [`studio.svg`](placeholders/studio.svg) | [Workflow studio](../visual-tour.md#workflow-studio) | `studio.jpg` | Placeholder |
| [`catalog.svg`](placeholders/catalog.svg) | [Node catalog](../visual-tour.md#node-catalog) | `catalog.jpg` | Placeholder |
| [`plugins.svg`](placeholders/plugins.svg) | [Runtime plugins](../visual-tour.md#runtime-plugins) | `plugins.jpg` | Placeholder |
| [`secrets.svg`](placeholders/secrets.svg) | [Secret metadata](../visual-tour.md#secret-metadata) | `secrets.jpg` | Placeholder |
| [`members.svg`](placeholders/members.svg) | [Organization members](../visual-tour.md#organization-members) | `members.jpg` | Placeholder |
| [`settings.svg`](placeholders/settings.svg) | [Workspace settings](../visual-tour.md#workspace-settings) | `settings.jpg` | Placeholder |
| [`mobile.svg`](placeholders/mobile.svg) | [Mobile management](../visual-tour.md#mobile-management) | `mobile-service-tokens.jpg` | Placeholder |

## Replacement instructions for Claude or another maintainer

1. Read the linked capture brief and use a running Corqty deployment. A screenshot
   must reflect actual rendered behavior; no image generation, HTML mockup, or
   drawn controls should be represented as a product capture.
2. Use a synthetic organization/account and harmless workflow data. Prefer
   `example.test` addresses. Never publish passwords, session cookies, invitation
   tokens, service tokens, secret values, webhook signing secrets, real customer
   data, or logs containing credentials. Dismiss one-time reveals first.
3. Prefer the documented viewport and 100% zoom. Keep enough navigation/context
   to identify the screen. Use full-page capture only when it improves the
   documented view; do not enlarge the browser merely to make a prettier image.
4. Capture through supported browser/computer tools or manually. If the tooling
   blocks capture, keep the placeholder and report that limit rather than
   bypassing it. Do not enable shell/network permissions or create live external
   integrations just to populate a screenshot.
5. Inspect the complete image at readable size. Confirm labels match the current
   guide, no loading/error/modal overlay obscures the intended state, and no
   credential is exposed. If it contains sensitive data, prepare a safe test
   state and recapture instead of hiding a real production context behind blur.
6. Save it under the proposed target filename. Update all references to that
   particular placeholder in Markdown only after the actual file exists. Keep
   relative links (`assets/screenshots/...` from a docs page), descriptive alt
   text, and a short factual caption.
7. Update this inventory with date, app/code revision if known, viewport,
   synthetic fixture description, and whether the image was cropped/processed.
   A new actual capture may use PNG instead of JPEG when text clarity benefits;
   update filename references consistently.
8. Check documentation image/source links and review rendered Markdown. Remove
   an unused placeholder only after every reference has been updated. Never
   leave a broken image link pointing to a future target.

## Diagram maintenance

Mermaid diagrams are source-level explanations, not screenshots. They show
role/product navigation, graph/definition/version relationships, draft
save/publish sequencing, frontend state ownership, and SSE replay. Update them
with the corresponding code contract. Keep labels concrete and avoid implying
features such as automatic draft merges, a separate frontend authorization
server, or unlimited live log storage.

SVG placeholder cards use only local vector/text content and system fonts.
They intentionally contain no application UI and no external dependencies.
