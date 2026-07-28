# HTTP Overhead Benchmark

This benchmark compares a generated Caelix controller with a matched plain
Actix or Axum handler. Every implementation serves the same `/hello` response,
generates a UUID for both correlation response headers, and runs in an isolated
release target directory.

Install [`wrk`](https://github.com/wg/wrk), then run:

```sh
benchmarks/http-overhead/scripts/run.sh
```

The runner builds each version/backend combination in an isolated target
directory, alternates their order over three runs, and records throughput and
latency in `results/summary.csv`. Configure it through `WRK`, `THREADS`,
`CONNECTIONS`, `DURATION`, `ROUNDS`, and `WORKERS` environment variables.
