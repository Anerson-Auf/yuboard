#!/usr/bin/env bash
# Deploy or control only Flowboard's two PM2 processes. It never restarts other services.
set -Eeuo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ECOSYSTEM="$ROOT_DIR/deploy/ecosystem.config.cjs"
ACTION="${1:-deploy}"

require_command() {
  command -v "$1" >/dev/null 2>&1 || { echo "Missing required command: $1" >&2; exit 1; }
}

cd "$ROOT_DIR"
require_command npm
require_command cargo
require_command pm2

case "$ACTION" in
  deploy)
    test -f .env || { echo "Create $ROOT_DIR/.env first; see docs/DEPLOY_PM2_CLOUDFLARE.md" >&2; exit 1; }
    echo "Installing locked Node dependencies…"
    npm ci
    echo "Building web…"
    npm run build
    echo "Building API…"
    CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-6}" cargo build --release --locked
    echo "Reloading only Flowboard processes…"
    pm2 startOrReload "$ECOSYSTEM" --only flowboard-web,flowboard-api --update-env
    pm2 save
    curl --fail --silent --show-error http://127.0.0.1:8100/health >/dev/null
    curl --fail --silent --show-error http://127.0.0.1:3100/ >/dev/null
    echo "Flowboard is running."
    ;;
  restart)
    pm2 restart flowboard-web flowboard-api --update-env
    ;;
  stop)
    pm2 stop flowboard-web flowboard-api
    ;;
  status)
    pm2 status flowboard-web flowboard-api
    ;;
  logs)
    pm2 logs flowboard-web flowboard-api
    ;;
  *)
    echo "Usage: $0 {deploy|restart|stop|status|logs}" >&2
    exit 2
    ;;
esac
