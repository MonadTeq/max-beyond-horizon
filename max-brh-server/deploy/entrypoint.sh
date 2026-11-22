#!/bin/sh
# M5.3: render the Horizon config from deploy/horizon.toml.template and launch the binary.
#
# All paths/addresses come from the docker-compose.yml environment — so the same
# image runs both locally (127.0.0.1) and on a public server (announce_host).
set -eu

CONFIG_TEMPLATE="${HORIZON_CONFIG_TEMPLATE:-/opt/horizon/horizon.toml.template}"
CONFIG_PATH="${HORIZON_CONFIG:-/tmp/horizon/config.toml}"

: "${HORIZON_ANNOUNCE_HOST:=127.0.0.1}"
: "${HORIZON_WS_BIND:=0.0.0.0:8080}"
: "${HORIZON_GRPC_BIND:=0.0.0.0:8082}"
: "${MAX_EXECUTABLE:=/opt/max/template/max-port}"
: "${MAX_TEMPLATE_DIR:=/opt/max/template}"
: "${MAX_SESSION_ROOT:=/var/lib/horizon/sessions}"
: "${MAX_GAME_DATA:=/opt/max/game-data}"
: "${MAX_PORT_START:=32000}"
: "${MAX_PORT_END:=32100}"
: "${MAX_RESTARTS:=0}"
: "${MAX_SESSION_SECRET:=smoke-secret}"
: "${MAX_ADMIN_SECRET:=smoke-admin}"

if [ ! -f "$CONFIG_TEMPLATE" ]; then
    echo "entrypoint: нет шаблона конфига $CONFIG_TEMPLATE" >&2
    exit 1
fi
if [ ! -f /opt/horizon/horizon ]; then
    echo "entrypoint: нет бинаря /opt/horizon/horizon (смонтируйте ./max-brh-server/target/release/horizon)" >&2
    exit 1
fi

# sed escape for '&', '|' and '\' in secrets/paths.
sed_escape() {
    printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'
}

mkdir -p "$(dirname "$CONFIG_PATH")" "$MAX_SESSION_ROOT"

sed \
    -e "s|@HORIZON_WS_BIND@|$(sed_escape "$HORIZON_WS_BIND")|g" \
    -e "s|@HORIZON_GRPC_BIND@|$(sed_escape "$HORIZON_GRPC_BIND")|g" \
    -e "s|@HORIZON_ANNOUNCE_HOST@|$(sed_escape "$HORIZON_ANNOUNCE_HOST")|g" \
    -e "s|@MAX_EXECUTABLE@|$(sed_escape "$MAX_EXECUTABLE")|g" \
    -e "s|@MAX_TEMPLATE_DIR@|$(sed_escape "$MAX_TEMPLATE_DIR")|g" \
    -e "s|@MAX_SESSION_ROOT@|$(sed_escape "$MAX_SESSION_ROOT")|g" \
    -e "s|@MAX_GAME_DATA@|$(sed_escape "$MAX_GAME_DATA")|g" \
    -e "s|@MAX_PORT_START@|$(sed_escape "$MAX_PORT_START")|g" \
    -e "s|@MAX_PORT_END@|$(sed_escape "$MAX_PORT_END")|g" \
    -e "s|@MAX_RESTARTS@|$(sed_escape "$MAX_RESTARTS")|g" \
    -e "s|@MAX_SESSION_SECRET@|$(sed_escape "$MAX_SESSION_SECRET")|g" \
    -e "s|@MAX_ADMIN_SECRET@|$(sed_escape "$MAX_ADMIN_SECRET")|g" \
    "$CONFIG_TEMPLATE" > "$CONFIG_PATH"

echo "entrypoint: конфиг $CONFIG_PATH (announce_host=$HORIZON_ANNOUNCE_HOST, UDP $MAX_PORT_START-$MAX_PORT_END)" >&2

# Binary copy: a bind-mount from Windows does not always preserve the exec bit.
cp /opt/horizon/horizon /tmp/horizon-bin
chmod +x /tmp/horizon-bin

exec /tmp/horizon-bin --config "$CONFIG_PATH" "$@"
