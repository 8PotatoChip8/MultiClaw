#!/bin/bash
set -e
for i in {1..3}; do
  if curl -s http://127.0.0.1:9999 > /dev/null; then
    break
  fi
  echo "sleeping"
  sleep 1
done
