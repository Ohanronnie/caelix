# HTTP Overhead Benchmark

This benchmark measures the HTTP overhead of a generated Caelix controller
against matched plain Actix and Axum handlers. Each pair uses the same runtime,
route, JSON response, and correlation response headers.

The run was performed on 2026-07-28. Results are specific to this machine and
load profile, so treat small differences as noise rather than a general runtime
ranking.

## Environment

- Apple M1 Pro with 10 logical CPUs and 16 GiB RAM
- macOS 27.0.0 (`arm64`)
- Rust 1.96.0
- `wrk` 4.2.0 (`kqueue`)
- Caelix `0.0.36`
- Actix Web 4.14.0 and Axum 0.8.9

## Configuration

Every binary served `GET /hello` and returned the same JSON string response.
Caelix used a generated controller. The plain handlers generated a UUID and
returned matching `x-request-id` and `x-trace-id` headers, status, content type,
and response body. The request contained the normal `Host` header but no
correlation headers.

- `wrk -t4 -c256 -d10s --latency`
- 2-second warm-up before each measurement
- 3 independently restarted runs per implementation, with each pair alternated
- Actix used 4 workers; Axum used its default multi-thread Tokio runtime
- Default Cargo release profile
- No socket errors or non-2xx responses were recorded

## Median results

<div class="benchmark-table-wrap">
  <table class="benchmark-results">
    <thead>
      <tr>
        <th>Backend</th>
        <th>Implementation</th>
        <th>Requests/s</th>
        <th>Delta</th>
        <th>p50</th>
        <th>p90</th>
        <th>p99</th>
      </tr>
    </thead>
    <tbody>
      <tr><th>Actix</th><td>Plain Actix</td><td>150,715</td><td>baseline</td><td>1.63 ms</td><td>1.80 ms</td><td>2.35 ms</td></tr>
      <tr><th>Actix</th><td><strong>Caelix Actix</strong></td><td><strong>151,959</strong></td><td>+0.83%</td><td><strong>1.64 ms</strong></td><td><strong>1.80 ms</strong></td><td><strong>2.14 ms</strong></td></tr>
      <tr><th>Axum</th><td>Plain Axum</td><td>152,687</td><td>baseline</td><td>1.55 ms</td><td>1.79 ms</td><td>2.54 ms</td></tr>
      <tr><th>Axum</th><td><strong>Caelix Axum</strong></td><td><strong>154,583</strong></td><td>+1.24%</td><td><strong>1.45 ms</strong></td><td><strong>1.87 ms</strong></td><td><strong>2.66 ms</strong></td></tr>
    </tbody>
  </table>
</div>

The median throughput difference is within 2% for both backends. On this
machine and load profile, Caelix is effectively at parity with the matched
plain handlers; treat the small positive differences as measurement noise.

## Reproduce

The benchmark source, runner, and measured CSV are in
[`benchmarks/http-overhead`](https://github.com/Ohanronnie/caelix/tree/main/benchmarks/http-overhead).
Install `wrk`, then run:

```sh
benchmarks/http-overhead/scripts/run.sh
```

The runner rebuilds isolated release binaries for all four implementations,
writes per-run output under `results/raw/`, and updates `results/summary.csv`.
