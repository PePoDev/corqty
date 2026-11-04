#!/usr/bin/env bash
# PostgreSQL roles for local development, integration tests and CI. The
# bootstrap superuser runs this once; corqty_owner then owns the database and
# runs migrations, seeds and test resets without superuser rights.
set -euo pipefail

psql -v ON_ERROR_STOP=1 \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set=owner_password="$CORQTY_OWNER_DB_PASSWORD" \
  --set=api_password="$CORQTY_API_DB_PASSWORD" \
  --set=worker_password="$CORQTY_WORKER_DB_PASSWORD" <<'SQL'
-- INHERIT: CREATE on schema public reaches the owner through pg_database_owner.
-- BYPASSRLS: the SECURITY DEFINER helpers it owns read FORCE RLS tables.
SELECT format(
  'CREATE ROLE corqty_owner LOGIN PASSWORD %L NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT BYPASSRLS',
  :'owner_password'
)
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'corqty_owner') \gexec

SELECT format('ALTER DATABASE %I OWNER TO corqty_owner', current_database()) \gexec

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
