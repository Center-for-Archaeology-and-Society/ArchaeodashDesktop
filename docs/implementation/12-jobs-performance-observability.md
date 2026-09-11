# IMPLEMENTATION Section 12 - Jobs, performance, and observability

> Split from `IMPLEMENTATION.md` on 2026-09-09. Section numbering matches the master plan, so cross-references such as `section 6.8` or `Section 8` remain valid. Return to the [master document](../../IMPLEMENTATION.md) for scope, principles, parity scope, test strategy, delivery phases, and the definition of done.

## 12. Jobs, performance, and observability

- Tokio owns asynchronous I/O; Rayon or a bounded CPU pool owns CPU-heavy dataframe/statistical work. Never run analysis on Axum request executors.
- Hosted jobs enforce per-user concurrency, queue length, memory, row/column/cell, output, and wall-clock quotas. Desktop uses machine-aware concurrency and remains responsive.
- Cancellation tokens are checked between imputation/permutation iterations, K values, distance blocks, cluster starts, and explicit result-export writes.
- Progress stages mirror useful current messages: validating, reading rows, normalizing schema, imputing, ratios, transforming, PCA, UMAP, LDA, persisting, publishing.
- Emit structured traces and metrics: latency/status by route/use case, job duration/queue time/cancellation, parse failures, group/source/result bytes, control-DB/file-store errors, active sessions, and readiness. Never use username, uploaded filename, or raw group/project name as metric labels.
- Use OpenTelemetry-compatible tracing, JSON server logs, rotating desktop logs, and an error taxonomy with stable user-safe codes.
- Health checks test liveness separately from readiness. External synthetic monitoring validates shell content and a safe authenticated test flow; container restart is orchestrator policy, not a shell script parsing HTML plus CPU.
- Add benchmarks for 10k/100k/1M rows where algorithms permit, wide groups, missingness, multi-group loads, candidate scans, Parquet validation/rewrite, Plotly point ceilings, top-k Euclidean memory, and result serialization.
- Benchmark the small-mutation throughput of the one-group-per-file cost model (Section 6.7): a 50-row group reassignment against 10k/100k/1M-row group files must complete staged rewrite, full validation, and journal commit within interactive latency budgets; publish the measured per-mutation cost with the benchmark ladder.
