# Performance budgets

CI fails on regressions over 10% against these targets.

| Metric | Budget |
|---|---|
| Base installer size | At most 30 MB per architecture |
| Cold start to interactive | At most 1.0 s on a mid-range x64 or Snapdragon X laptop |
| Idle RAM including WebView2 | At most 150 MB |
| First thumbnail | At most 300 ms |
| Merge of 100 small PDFs | Under 3 s |

## Concurrency and memory

- All processing runs off the UI thread.
- Bounded concurrency: physical cores minus 1, at most 4 heavy jobs.
- Page-at-a-time streaming for large files, cancellation checked per page.
- Worker default memory cap 2 GB inside the Job Object.

## Measurement

Benchmarks live next to the code they measure and are run on both architectures before each release (arm64 natively).
