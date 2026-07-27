#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT/Cargo.toml"
WRK="${WRK:-wrk}"
THREADS="${THREADS:-4}"
CONNECTIONS="${CONNECTIONS:-256}"
DURATION="${DURATION:-10s}"
ROUNDS="${ROUNDS:-3}"
WORKERS="${WORKERS:-4}"
RESULTS="${RESULTS:-$ROOT/results}"
TARGET="$ROOT/target"
SUMMARY="$RESULTS/summary.csv"

command -v "$WRK" >/dev/null || {
  echo "wrk is required; set WRK to its executable path" >&2
  exit 1
}

mkdir -p "$RESULTS/raw" "$RESULTS/server-logs"
printf 'round,position,implementation,rps,p50,p90,p99,socket_errors,non_2xx\n' >"$SUMMARY"

build() {
  local implementation="$1"
  CARGO_TARGET_DIR="$TARGET/$implementation" cargo build \
    --manifest-path "$MANIFEST" \
    --release \
    --no-default-features \
    --features "$implementation"
}

for implementation in baseline-actix current-actix baseline-axum current-axum; do
  build "$implementation"
done

server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

run_one() {
  local round="$1"
  local position="$2"
  local implementation="$3"
  local binary="$TARGET/$implementation/release/caelix-correlation-header-benchmark"
  local url="http://127.0.0.1:4101/hello"
  local raw="$RESULTS/raw/round-${round}-${position}-${implementation}.txt"
  local log="$RESULTS/server-logs/round-${round}-${position}-${implementation}.txt"

  BENCH_WORKERS="$WORKERS" "$binary" >"$log" 2>&1 &
  server_pid=$!
  for _ in $(seq 1 100); do
    curl --fail --silent "$url" >/dev/null && break
    sleep 0.05
  done
  curl --fail --silent "$url" >/dev/null || {
    echo "server did not become ready: $implementation" >&2
    return 1
  }

  "$WRK" -t2 -c100 -d2s "$url" >/dev/null
  "$WRK" -t"$THREADS" -c"$CONNECTIONS" -d"$DURATION" --latency "$url" >"$raw"

  local rps p50 p90 p99 socket_errors non_2xx
  rps="$(awk '/Requests\/sec:/ { print $2 }' "$raw")"
  p50="$(awk '$1 == "50%" { print $2 }' "$raw")"
  p90="$(awk '$1 == "90%" { print $2 }' "$raw")"
  p99="$(awk '$1 == "99%" { print $2 }' "$raw")"
  socket_errors="$(awk '/Socket errors:/ { sub(/^[[:space:]]*/, ""); print }' "$raw" | tr ',' ';')"
  non_2xx="$(awk '/Non-2xx or 3xx responses:/ { print $NF }' "$raw")"
  printf '%s,%s,%s,%s,%s,%s,%s,"%s",%s\n' \
    "$round" "$position" "$implementation" "$rps" "$p50" "$p90" "$p99" \
    "$socket_errors" "${non_2xx:-0}" >>"$SUMMARY"

  cleanup
  server_pid=""
}

orders=(
  "baseline-actix current-actix baseline-axum current-axum"
  "current-axum baseline-axum current-actix baseline-actix"
  "baseline-axum current-axum baseline-actix current-actix"
)

for round in $(seq 1 "$ROUNDS"); do
  read -r -a order <<<"${orders[$(((round - 1) % ${#orders[@]}))]}"
  for position in "${!order[@]}"; do
    run_one "$round" "$((position + 1))" "${order[$position]}"
  done
done

echo "summary=$SUMMARY"
