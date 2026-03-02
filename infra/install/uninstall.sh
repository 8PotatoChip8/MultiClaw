#!/usr/bin/env bash

set -e

echo "Uninstalling MultiClaw..."
docker compose -f /opt/multiclaw/docker-compose.yml down -v || echo "Warn: compose down failed"
rm -rf /opt/multiclaw
rm -rf /var/lib/multiclaw

echo "Cleaning up incus vms..."
incus list --format csv -c n | grep '^mc-' | xargs -r incus delete --force

echo "MultiClaw Uninstalled."
