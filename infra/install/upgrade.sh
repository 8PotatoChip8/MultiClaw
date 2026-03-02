#!/usr/bin/env bash

set -e
echo "Upgrading MultiClaw Components..."
docker compose -f /opt/multiclaw/docker-compose.yml pull
docker compose -f /opt/multiclaw/docker-compose.yml up -d

echo "Upgrade Complete."
