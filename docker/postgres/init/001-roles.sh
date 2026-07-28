#!/usr/bin/env bash
# PostgreSQL runtime roles for local development and integration tests.
set -euo pipefail

psql \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set=api_password="$CORQTY_API_DB_PASSWORD" \
  --set=worker_password="$CORQTY_WORKER_DB_PASSWORD" <<'SQL'
SELECT format(
  'CREATE ROLE corqty_api LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS',
  :'api_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'corqty_api') \gexec

SELECT format(
  'CREATE ROLE corqty_worker LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS',
  :'worker_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'corqty_worker') \gexec

SELECT format(
  'GRANT CONNECT ON DATABASE %I TO corqty_api, corqty_worker',
  current_database()
) \gexec
SQL
