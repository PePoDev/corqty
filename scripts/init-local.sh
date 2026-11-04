#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
if [ -e .env ]; then
    printf '%s\n' '.env already exists; preserved. Set CORQTY_MASTER_KEY if upgrading.'
    exit 0
fi
umask 077
cp .env.example .env
for old in postgres_dev corqty_owner_dev corqty_api_dev corqty_worker_dev; do
    value=$(openssl rand -hex 24)
    sed "s/$old/$value/g" .env > .env.initializing
    mv .env.initializing .env
done
key=$(openssl rand -base64 32)
sed "s|^CORQTY_MASTER_KEY=$|CORQTY_MASTER_KEY=$key|" .env > .env.initializing
mv .env.initializing .env
printf '%s\n' 'Created .env with fresh local credentials. Back it up securely.'
