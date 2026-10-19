#!/usr/bin/env bash
# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

# Builds the website and copies it to the server. With --caddy it also installs the Caddy site
# block and reloads Caddy. The port and user come from ~/.ssh/config. See README.md.
#
#   site/deploy.sh [--caddy]
set -euo pipefail

server=${ARIEKEI_SERVER:-ariekei.com}
cd "$(dirname "$0")"

export ASTRO_TELEMETRY_DISABLED=1
npm ci
npm run build

rsync -rlz --delete --chmod=D755,F644 dist/ "$server:/var/www/ariekei/"

if [[ ${1:-} == --caddy ]]; then
  ssh "$server" 'sudo tee /etc/caddy/sites/ariekei.caddy >/dev/null' < deploy/ariekei.caddy
  ssh "$server" 'sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile && sudo systemctl reload caddy'
fi
echo "Deployed to https://ariekei.com/"
