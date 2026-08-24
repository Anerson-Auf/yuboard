#!/usr/bin/env bash
# Runs a locally-managed Cloudflare Tunnel without putting credentials in PM2 args.
set -Eeuo pipefail

CLOUDFLARED_BIN="${FLOWBOARD_CLOUDFLARED_BIN:-$HOME/cloudflared}"
TUNNEL_CONFIG="${FLOWBOARD_TUNNEL_CONFIG:-$HOME/.cloudflared/flowboard.yml}"
TUNNEL_NAME="${FLOWBOARD_TUNNEL_NAME:-flowboard}"

test -x "$CLOUDFLARED_BIN" || { echo "cloudflared is not executable: $CLOUDFLARED_BIN" >&2; exit 1; }
test -f "$TUNNEL_CONFIG" || { echo "Tunnel config is absent: $TUNNEL_CONFIG" >&2; exit 1; }

exec "$CLOUDFLARED_BIN" --config "$TUNNEL_CONFIG" tunnel run "$TUNNEL_NAME"
