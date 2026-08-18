#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
project_dir=$(cd -- "$script_dir/.." && pwd)
var_dir="$project_dir/var"
data_dir="$var_dir/postgres"
socket_dir="$var_dir/postgres-socket"
log_file="$var_dir/postgres.log"
port=${BENCH_PG_PORT:-55432}

if [[ -n "${BENCH_PG_BIN:-}" ]]; then
    pg_bin=$BENCH_PG_BIN
elif command -v pg_config >/dev/null 2>&1; then
    pg_bin=$(pg_config --bindir)
else
    echo "PostgreSQL server tools were not found. Set BENCH_PG_BIN to their bin directory." >&2
    exit 1
fi

initdb="$pg_bin/initdb"
pg_ctl="$pg_bin/pg_ctl"

if [[ ! -x "$initdb" || ! -x "$pg_ctl" ]]; then
    echo "initdb and pg_ctl were not found in $pg_bin" >&2
    exit 1
fi

initialize() {
    mkdir -p "$var_dir" "$socket_dir"
    if [[ ! -f "$data_dir/PG_VERSION" ]]; then
        "$initdb" -D "$data_dir" --username=postgres --auth=trust
    fi
}

is_running() {
    [[ -f "$data_dir/PG_VERSION" ]] && "$pg_ctl" status -D "$data_dir" >/dev/null 2>&1
}

start() {
    initialize
    if ! is_running; then
        "$pg_ctl" -D "$data_dir" -l "$log_file" \
            -o "-h '' -p $port -k '$socket_dir'" -w start
    fi
    if [[ $(psql -h "$socket_dir" -p "$port" -U postgres -d postgres -tAc \
        "SELECT 1 FROM pg_database WHERE datname = 'benchmark'") != "1" ]]; then
        createdb -h "$socket_dir" -p "$port" -U postgres benchmark
    fi
    echo "PostgreSQL is ready on $socket_dir at port $port"
}

stop() {
    if is_running; then
        "$pg_ctl" -D "$data_dir" -w stop -m fast
    else
        echo "PostgreSQL is not running"
    fi
}

status() {
    if is_running; then
        "$pg_ctl" status -D "$data_dir"
    else
        echo "PostgreSQL is not running"
        return 1
    fi
}

case "${1:-}" in
    init)
        initialize
        ;;
    start)
        start
        ;;
    stop)
        stop
        ;;
    status)
        status
        ;;
    url)
        echo "socket=$socket_dir port=$port user=postgres dbname=benchmark"
        ;;
    *)
        echo "Usage: $0 {init|start|stop|status|url}" >&2
        exit 2
        ;;
esac
