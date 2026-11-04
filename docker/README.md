# Docker layout

The repository keeps one application Compose entry point at its root. Supporting
container files live here:

```text
docker-compose.yml          Local application: PostgreSQL, migrator, API, worker, web
docker/
  compose.production.yml    HTTPS edge and production overrides
  compose.test.yml          Disposable integration PostgreSQL on port 55432
  Caddyfile                 Public HTTPS forwarding to web
  postgres/init/            Database role initialization
  rust/Dockerfile           Rust builder and API/worker/migrator runtime targets
  web/Dockerfile            Production standalone Next.js image
```

Run commands from the repository root:

```sh
docker compose up --build --wait
docker compose -f docker-compose.yml -f docker/compose.production.yml up --build --wait
docker compose -f docker/compose.test.yml up -d --wait
```

These are alternative workflows, not commands to run blindly in sequence.
Initialize local credentials first; production additionally requires
`CORQTY_DOMAIN`, DNS, and open HTTPS ports. The production file is an override,
so the root file must be listed first. Its paths use the root file's project
directory. The test file is standalone: its initialization mount is relative
to this `docker/` directory, and its explicit project name preserves `corqty`.

Docker build contexts remain the repository root even though Dockerfiles live
here. Relative `COPY` sources therefore refer to root files such as `crates/`
and `apps/web/`. The application database uses a named volume; test PostgreSQL
is disposable. Do not use volume deletion as an upgrade procedure.

See [configuration](../docs/configuration.md), [operations](../docs/operations.md),
and [testing](../docs/testing.md) for complete instructions and trust boundaries.
