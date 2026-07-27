# Results

Caelix HTTP overhead benchmark run on 2026-07-27 against Caelix `0.0.33`.

## Environment

- Ubuntu 24.04.3 on Linux 6.12.13 (`x86_64`)
- AMD EPYC 9V74
- 9 logical CPUs visible, with an 8-CPU cgroup quota
- 15 GiB RAM and no swap
- Rust 1.97.1
- `wrk` 4.1.0 from Ubuntu packages

## Configuration

- Caelix 0.0.33, Actix Web 4.14.0, and Axum 0.8.9
- 8 server workers/runtime threads for every binary
- `wrk -t8 -c1000 -d30s --latency`
- 3-second warm-up before each measurement
- 3 independently restarted runs, with run order alternated
- Actix access logging disabled
- Release profile: `opt-level=3`, thin LTO, one codegen unit, aborting panic,
  and stripped binaries

Every app served `GET /hello` with:

```json
{ "message": "Hello, world!" }
```

Caelix adds matching `x-request-id` and `x-trace-id` UUID headers by default.
The plain Actix and Axum handlers explicitly performed the same UUID generation
and returned the same headers, status, JSON body, content type, and content
length.

## Median results

<div class="benchmark-table-wrap">
  <table class="benchmark-results">
    <thead>
      <tr>
        <th>Pair</th>
        <th>Implementation</th>
        <th>Requests/s</th>
        <th>Delta</th>
        <th>p50</th>
        <th>p90</th>
        <th>p99</th>
        <th>RSS after</th>
      </tr>
    </thead>
    <tbody>
      <tr><th>Actix</th><td>Plain Actix</td><td>644,724</td><td>baseline</td><td>0.777 ms</td><td>5.63 ms</td><td>13.88 ms</td><td>17.78 MB</td></tr>
      <tr><th>Actix</th><td><strong>Caelix Actix</strong></td><td><strong>586,480</strong></td><td>-9.03%</td><td><strong>0.990 ms</strong></td><td><strong>5.80 ms</strong></td><td><strong>14.92 ms</strong></td><td><strong>17.74 MB</strong></td></tr>
      <tr><th>Axum</th><td>Plain Axum</td><td>467,921</td><td>baseline</td><td>1.800 ms</td><td>4.19 ms</td><td>9.99 ms</td><td>22.72 MB</td></tr>
      <tr><th>Axum</th><td><strong>Caelix Axum</strong></td><td><strong>359,737</strong></td><td>-23.12%</td><td><strong>2.480 ms</strong></td><td><strong>5.01 ms</strong></td><td><strong>9.91 ms</strong></td><td><strong>19.86 MB</strong></td></tr>
    </tbody>
  </table>
</div>

Mean throughput deltas were -8.91% for Caelix Actix and -22.38% for Caelix
Axum. The direction did not change when order was reversed.

One socket read error occurred during one Caelix Axum run, across approximately
32.8 million Caelix Axum requests. No non-2xx responses were reported.

RSS with 1,000 connections is allocator-sensitive. Actix memory was effectively
equal. Axum RSS varied enough between runs that its apparent Caelix advantage
should not be treated as a firm memory result.

## Throughput runs

<div class="benchmark-table-wrap">
  <table class="benchmark-results benchmark-throughput-runs">
    <thead>
      <tr>
        <th>Implementation</th>
        <th>Run 1</th>
        <th>Run 2</th>
        <th>Run 3</th>
        <th>Median</th>
        <th>Mean</th>
      </tr>
    </thead>
    <tbody>
      <tr><th>Plain Actix</th><td>645,111</td><td>638,400</td><td>644,724</td><td>644,724</td><td>642,745</td></tr>
      <tr><th>Caelix Actix</th><td>573,916</td><td>595,952</td><td>586,480</td><td>586,480</td><td>585,450</td></tr>
      <tr><th>Plain Axum</th><td>467,921</td><td>465,991</td><td>471,055</td><td>467,921</td><td>468,322</td></tr>
      <tr><th>Caelix Axum</th><td>371,998</td><td>359,737</td><td>358,825</td><td>359,737</td><td>363,520</td></tr>
    </tbody>
  </table>
</div>
