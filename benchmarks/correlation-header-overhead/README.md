# Correlation Header Validation Benchmark

This focused benchmark compares generated routes from published Caelix `0.0.35`
against the current workspace source. It covers both Actix and Axum with a
minimal controller that exercises correlation-header validation on every
request.

Install [`wrk`](https://github.com/wg/wrk), then run:

```sh
benchmarks/correlation-header-overhead/scripts/run.sh
```

The runner builds each version/backend combination in an isolated target
directory, alternates their order over three runs, and records throughput and
latency in `results/summary.csv`. Configure it through `WRK`, `THREADS`,
`CONNECTIONS`, `DURATION`, `ROUNDS`, and `WORKERS` environment variables.
