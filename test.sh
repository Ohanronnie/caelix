#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
exec "$ROOT/benchmarks/correlation-header-overhead/scripts/run.sh" "$@"
