#!/usr/bin/env bash
set -euo pipefail

ulimit -n 4096

ROOT="$(cd "$(dirname "$0")" && pwd)"
WRK="${WRK:-$ROOT/../wrk-package/usr/bin/wrk}"
export LD_LIBRARY_PATH="$ROOT/../wrk-package/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
WORKERS="${WORKERS:-8}"
THREADS="${THREADS:-8}"
CONNECTIONS="${CONNECTIONS:-1000}"
DURATION="${DURATION:-30s}"
ROUNDS="${ROUNDS:-3}"
RESULTS="$ROOT/results"
SUMMARY="$RESULTS/summary.csv"

mkdir -p "$RESULTS/raw" "$RESULTS/server-logs"
printf 'round,position,name,port,rps,p50,p90,p99,rss_before_kb,rss_after_kb,socket_errors,non_2xx\n' >"$SUMMARY"

declare -A PORTS=(
  [plain-actix]=4102
  [caelix-actix-bench]=4101
  [plain-axum]=4202
  [caelix-axum-bench]=4201
)

orders=(
  "plain-actix caelix-actix-bench plain-axum caelix-axum-bench"
  "caelix-axum-bench plain-axum caelix-actix-bench plain-actix"
  "plain-axum caelix-axum-bench plain-actix caelix-actix-bench"
  "caelix-actix-bench plain-actix caelix-axum-bench plain-axum"
  "plain-actix caelix-actix-bench plain-axum caelix-axum-bench"
)

server_pid=""
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

rss_kb() {
  awk '/^VmRSS:/ { print $2 }' "/proc/$1/status"
}

run_one() {
  local round="$1"
  local position="$2"
  local name="$3"
  local port="${PORTS[$name]}"
  local raw="$RESULTS/raw/round-${round}-${position}-${name}.txt"
  local server_log="$RESULTS/server-logs/round-${round}-${position}-${name}.txt"
  local url="http://127.0.0.1:${port}/hello"
  local host_pid

  echo "round=$round position=$position app=$name port=$port"
  BENCH_WORKERS="$WORKERS" "$ROOT/target/release/$name" >"$server_log" 2>&1 &
  server_pid=$!

  local ready=0
  for _ in $(seq 1 100); do
    if curl --fail --silent "$url" >/dev/null; then
      ready=1
      break
    fi
    sleep 0.05
  done
  if [[ "$ready" -ne 1 ]]; then
    echo "server did not become ready: $name" >&2
    return 1
  fi
  host_pid="$(pgrep -f "^${ROOT}/target/release/${name}$" | head -n 1)"

  "$WRK" -t2 -c100 -d3s "$url" >/dev/null
  local rss_before
  rss_before="$(rss_kb "$host_pid")"

  "$WRK" -t"$THREADS" -c"$CONNECTIONS" -d"$DURATION" --latency "$url" >"$raw"

  local rss_after rps p50 p90 p99 socket_errors non_2xx
  rss_after="$(rss_kb "$host_pid")"
  rps="$(awk '/Requests\/sec:/ {print $2}' "$raw")"
  p50="$(awk '$1 == "50%" {print $2}' "$raw")"
  p90="$(awk '$1 == "90%" {print $2}' "$raw")"
  p99="$(awk '$1 == "99%" {print $2}' "$raw")"
  socket_errors="$(awk '/Socket errors:/ {print $0}' "$raw" | sed 's/^[[:space:]]*//' | tr ',' ';')"
  non_2xx="$(awk '/Non-2xx or 3xx responses:/ {print $NF}' "$raw")"

  printf '%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,"%s",%s\n' \
    "$round" "$position" "$name" "$port" "$rps" "$p50" "$p90" "$p99" \
    "$rss_before" "$rss_after" "$socket_errors" "$non_2xx" >>"$SUMMARY"

  echo "  rps=$rps p50=$p50 p99=$p99 rss=${rss_before}->${rss_after}KB"
  cleanup
  server_pid=""
}

echo "workers=$WORKERS threads=$THREADS connections=$CONNECTIONS duration=$DURATION rounds=$ROUNDS"
for round in $(seq 1 "$ROUNDS"); do
  read -r -a apps <<<"${orders[$((round - 1))]}"
  position=0
  for name in "${apps[@]}"; do
    position=$((position + 1))
    run_one "$round" "$position" "$name"
  done
done

echo "summary=$SUMMARY"
