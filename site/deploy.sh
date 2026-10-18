#!/usr/bin/env bash
# Copyright contributors to Ariekei.
# SPDX-License-Identifier: Apache-2.0

# Builds the website and copies it to the server. With --caddy it
# also installs the Caddy site block and reloads Caddy. See README.md.
#
#   ARIEKEI_DOMAIN=example.org site/deploy.sh [--caddy]
set -euo pipefail

domain=${ARIEKEI_DOMAIN:?set ARIEKEI_DOMAIN to the domain of the site}
server=${ARIEKEI_SERVER:-ariekei.com}
ssh=(ssh)
cd "$(dirname "$0")"

export ASTRO_TELEMETRY_DISABLED=1
npm ci
ARIEKEI_DOMAIN=$domain npm run build

rsync -rlz --delete --chmod=D755,F644 -e "${ssh[*]}" dist/ "$server:/var/www/ariekei/"

if [[ ${1:-} == --caddy ]]; then
  sed "s/ARIEKEI_DOMAIN/$domain/" deploy/ariekei.caddy \
    | "${ssh[@]}" "$server" 'sudo tee /etc/caddy/sites/ariekei.caddy >/dev/null'
  "${ssh[@]}" "$server" 'sudo caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile && sudo systemctl reload caddy'
fi
echo "Deployed to https://$domain/"
