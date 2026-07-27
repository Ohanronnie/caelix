# Correlation Header Validation Benchmark

This benchmark measures the generated-route change in Caelix `0.0.36` that
validates request headers and detects duplicate correlation headers in one
`HeaderMap::iter()` pass. It compares the published `0.0.35` implementation
with `0.0.36`; it is not a comparison with a plain Actix or Axum application.

The run was performed on 2026-07-27. Results are specific to this machine and
load profile, so treat small differences as noise rather than a general runtime
ranking.

## Environment

- Apple M1 Pro with 10 logical CPUs and 16 GiB RAM
- macOS 27.0.0 (`arm64`)
- Rust 1.96.0
- `wrk` 4.2.0 (`kqueue`)
- Caelix `0.0.35` baseline and Caelix `0.0.36` candidate
- Actix Web 4.14.0 and Axum 0.8.9

## Configuration

Every binary served `GET /hello` through a generated Caelix controller and
returned the same JSON string response. The request contained the normal `Host`
header but no correlation headers, so each route performed the production
header validation and generated correlation identifiers.

- `wrk -t4 -c256 -d10s --latency`
- 2-second warm-up before each measurement
- 3 independently restarted runs per implementation
- Actix used 4 workers; Axum used its default multi-thread Tokio runtime
- Default Cargo release profile
- Run order alternated between the baseline and candidate for each backend

## Median results

<div class="benchmark-table-wrap">
  <table class="benchmark-results">
    <thead>
      <tr>
        <th>Backend</th>
        <th>Caelix version</th>
        <th>Requests/s</th>
        <th>Delta</th>
        <th>p50</th>
        <th>p90</th>
        <th>p99</th>
      </tr>
    </thead>
    <tbody>
      <tr><th>Actix</th><td>0.0.35</td><td>153,560</td><td>baseline</td><td>1.63 ms</td><td>1.77 ms</td><td>2.02 ms</td></tr>
      <tr><th>Actix</th><td><strong>0.0.36</strong></td><td><strong>152,761</strong></td><td>-0.52%</td><td><strong>1.64 ms</strong></td><td><strong>1.77 ms</strong></td><td><strong>2.05 ms</strong></td></tr>
      <tr><th>Axum</th><td>0.0.35</td><td>153,908</td><td>baseline</td><td>1.48 ms</td><td>1.81 ms</td><td>2.33 ms</td></tr>
      <tr><th>Axum</th><td><strong>0.0.36</strong></td><td><strong>154,185</strong></td><td>+0.18%</td><td><strong>1.47 ms</strong></td><td><strong>1.86 ms</strong></td><td><strong>2.48 ms</strong></td></tr>
    </tbody>
  </table>
</div>

The median throughput difference is below 1% for both backends, so this run
finds the optimization neutral. It preserves the duplicate-correlation-header
protection while removing three `get_all()` searches and a separate validation
pass from every generated route request.

## Reproduce

The benchmark source, baseline lockfile, runner, and measured CSV are in
[`benchmarks/correlation-header-overhead`](https://github.com/Ohanronnie/caelix/tree/main/benchmarks/correlation-header-overhead).
Install `wrk`, then run:

```sh
benchmarks/correlation-header-overhead/scripts/run.sh
```

The runner rebuilds isolated release binaries for the baseline and current
source, writes per-run output under `results/raw/`, and updates
`results/summary.csv`.
