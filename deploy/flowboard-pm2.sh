#!/usr/bin/env bash
# Deploy or control only Flowboard's two PM2 processes. It never restarts other services.
set -Eeuo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ECOSYSTEM="$ROOT_DIR/deploy/ecosystem.config.cjs"
ACTION="${1:-deploy}"

# Keep this app separate from PM2 instances that supervise unrelated services.
export PM2_HOME="${PM2_HOME:-$HOME/.pm2-flowboard}"

require_command() {
  command -v "$1" >/dev/null 2>&1 || { echo "Missing required command: $1" >&2; exit 1; }
}

grant_nginx_static_access() {
  # Nginx may serve hashed client assets directly while the application itself
  # remains private to the deploy user. A clean build recreates `dist`, so ACLs
  # must be restored after every build. Silently skip non-Linux/dev systems.
  local nginx_user="${FLOWBOARD_NGINX_STATIC_USER:-www-data}"
  local static_dir="$ROOT_DIR/dist/client/_next/static"

  command -v setfacl >/dev/null 2>&1 || return 0
  id "$nginx_user" >/dev/null 2>&1 || return 0
  test -d "$static_dir" || return 0

  echo "Granting $nginx_user read-only access to generated web assets…"
  setfacl -m "u:${nginx_user}:--x" \
    "$HOME" \
    "$ROOT_DIR" \
    "$ROOT_DIR/dist" \
    "$ROOT_DIR/dist/client" \
    "$ROOT_DIR/dist/client/_next"
  setfacl -R -m "u:${nginx_user}:rX" "$static_dir"
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
    echo "Removing the previous generated web build and Vinext cache…"
    rm -rf "$ROOT_DIR/dist" "$ROOT_DIR/.vinext"
    echo "Building web…"
    npm run build
    grant_nginx_static_access
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
  tunnel)
    test -f "$HOME/.cloudflared/flowboard.yml" || { echo "Create the tunnel config first; see docs/DEPLOY_CLOUDFLARE_TUNNEL.md" >&2; exit 1; }
    pm2 startOrReload "$ECOSYSTEM" --only flowboard-tunnel --update-env
    pm2 save
    ;;
  *)
    echo "Usage: $0 {deploy|restart|stop|status|logs|tunnel}" >&2
    exit 2
    ;;
esac
