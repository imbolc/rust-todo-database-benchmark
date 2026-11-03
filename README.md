# PostgreSQL vs SQLite vs Turso

This project benchmarks three local databases through five Rust adapters and
six runnable configurations:

| Database   | Adapter                            | Async |
| ---------- | ---------------------------------- | ----- |
| PostgreSQL | `tokio-postgres`                   | Yes   |
| PostgreSQL | SQLx                               | Yes   |
| SQLite     | `rusqlite`                         | No    |
| SQLite     | SQLx                               | Yes   |
| Turso      | Native `turso` crate, WAL and MVCC | Yes   |

`Async` describes the driver API used by the benchmark. SQLx exposes an async
SQLite API by dispatching SQLite operations to a worker thread; SQLite itself
remains synchronous.

The fixture models users, todos, user-owned tags, and a many-to-many `todo_tags`
relation. All database files, PostgreSQL data, logs, and benchmark results are
kept in `./var`.

## Latest local results

These results are the per-metric medians of three release-mode trials per
durability and journal-mode combination, run on 2026-08-17 and 2026-08-18.
Every trial covered all six workloads at both concurrency levels. PostgreSQL
appears once because the two benchmark profiles do not change its durability
settings. The JSON reports under `var` also contain p50, p95, maximum latency,
and operation counts.

### Selected async configurations

| Workload      | Adapter          | Async | Mode           | 1 client ops/s | p99 (us) | 8 clients ops/s | p99 (us) |
| ------------- | ---------------- | ----- | -------------- | -------------: | -------: | --------------: | -------: |
| Basic write   | PostgreSQL, SQLx | Yes   | Server default |        2,008.6 |    1,086 |        11,562.2 |    1,400 |
| Basic write   | SQLite, SQLx     | Yes   | `NORMAL`       |       13,131.4 |      159 |        21,943.6 |    5,151 |
| Basic write   | Turso, WAL       | Yes   | `NORMAL`       |       40,150.0 |       46 |        21,393.0 |      109 |
| Basic write   | Turso, MVCC      | Yes   | `NORMAL`       |          269.2 |   14,087 |           953.6 |   32,207 |
| Fan-out write | PostgreSQL, SQLx | Yes   | Server default |        1,523.8 |    1,465 |         8,039.4 |    2,405 |
| Fan-out write | SQLite, SQLx     | Yes   | `NORMAL`       |        3,791.4 |    4,447 |         5,111.8 |    7,159 |
| Fan-out write | Turso, WAL       | Yes   | `NORMAL`       |       10,399.2 |    2,002 |         5,838.6 |    3,409 |
| Fan-out write | Turso, MVCC      | Yes   | `NORMAL`       |        6,650.8 |      156 |        24,726.2 |      616 |
| Retag         | PostgreSQL, SQLx | Yes   | Server default |        1,486.4 |    1,390 |         8,075.6 |    2,385 |
| Retag         | SQLite, SQLx     | Yes   | `NORMAL`       |        5,376.0 |    1,482 |         5,634.8 |    6,595 |
| Retag         | Turso, WAL       | Yes   | `NORMAL`       |       14,390.8 |      160 |         6,938.8 |    1,524 |
| Retag         | Turso, MVCC      | Yes   | `NORMAL`       |          687.2 |   21,631 |           411.6 |   46,239 |
| Indexed read  | PostgreSQL, SQLx | Yes   | Server default |       19,436.4 |      136 |        59,250.2 |      276 |
| Indexed read  | SQLite, SQLx     | Yes   | `NORMAL`       |       24,326.4 |       88 |        13,144.0 |    1,040 |
| Indexed read  | Turso, WAL       | Yes   | `NORMAL`       |       50,085.2 |       34 |       224,039.2 |       79 |
| Indexed read  | Turso, MVCC      | Yes   | `NORMAL`       |       26,013.6 |       59 |        76,220.0 |      226 |
| Join read     | PostgreSQL, SQLx | Yes   | Server default |        8,747.8 |      215 |        23,922.4 |      633 |
| Join read     | SQLite, SQLx     | Yes   | `NORMAL`       |       10,944.0 |      190 |         4,663.4 |    2,941 |
| Join read     | Turso, WAL       | Yes   | `NORMAL`       |        7,759.0 |      187 |        36,464.8 |      407 |
| Join read     | Turso, MVCC      | Yes   | `NORMAL`       |        4,495.8 |      334 |        15,953.6 |    1,101 |
| Balanced      | PostgreSQL, SQLx | Yes   | Server default |        3,850.8 |      922 |        17,049.4 |    1,498 |
| Balanced      | SQLite, SQLx     | Yes   | `NORMAL`       |       10,129.6 |      235 |         9,742.8 |   13,175 |
| Balanced      | Turso, WAL       | Yes   | `NORMAL`       |       18,069.2 |      176 |        13,480.4 |    8,167 |
| Balanced      | Turso, MVCC      | Yes   | `NORMAL`       |        3,814.2 |    2,321 |        11,146.6 |    5,951 |

### Detailed results

| Workload      | Adapter                      | Async | Mode           | 1 client ops/s | p99 (us) | 8 clients ops/s | p99 (us) |
| ------------- | ---------------------------- | ----- | -------------- | -------------: | -------: | --------------: | -------: |
| Basic write   | PostgreSQL, `tokio-postgres` | Yes   | Server default |        1,901.8 |    1,249 |        11,622.8 |    1,205 |
| Basic write   | PostgreSQL, SQLx             | Yes   | Server default |        2,008.6 |    1,086 |        11,562.2 |    1,400 |
| Basic write   | SQLite, `rusqlite`           | No    | `FULL`         |        1,798.2 |    1,806 |         1,784.0 |    2,283 |
| Basic write   | SQLite, `rusqlite`           | No    | `NORMAL`       |       17,756.0 |       48 |        59,983.4 |       56 |
| Basic write   | SQLite, SQLx                 | Yes   | `FULL`         |        1,525.0 |    2,181 |         1,373.0 |   55,391 |
| Basic write   | SQLite, SQLx                 | Yes   | `NORMAL`       |       13,131.4 |      159 |        21,943.6 |    5,151 |
| Basic write   | Turso, WAL                   | Yes   | `FULL`         |          968.4 |    2,737 |         1,353.6 |    2,273 |
| Basic write   | Turso, WAL                   | Yes   | `NORMAL`       |       40,150.0 |       46 |        21,393.0 |      109 |
| Basic write   | Turso, MVCC                  | Yes   | `FULL`         |          973.4 |    2,507 |         1,655.2 |   18,335 |
| Basic write   | Turso, MVCC                  | Yes   | `NORMAL`       |          269.2 |   14,087 |           953.6 |   32,207 |
| Fan-out write | PostgreSQL, `tokio-postgres` | Yes   | Server default |        1,517.6 |    1,558 |         8,303.8 |    2,191 |
| Fan-out write | PostgreSQL, SQLx             | Yes   | Server default |        1,523.8 |    1,465 |         8,039.4 |    2,405 |
| Fan-out write | SQLite, `rusqlite`           | No    | `FULL`         |        1,326.2 |    2,885 |         1,273.0 |    5,039 |
| Fan-out write | SQLite, `rusqlite`           | No    | `NORMAL`       |        5,269.2 |    4,427 |        16,250.2 |      389 |
| Fan-out write | SQLite, SQLx                 | Yes   | `FULL`         |        1,188.2 |    3,137 |         1,018.2 |   56,127 |
| Fan-out write | SQLite, SQLx                 | Yes   | `NORMAL`       |        3,791.4 |    4,447 |         5,111.8 |    7,159 |
| Fan-out write | Turso, WAL                   | Yes   | `FULL`         |          784.8 |    3,639 |         1,055.2 |    4,223 |
| Fan-out write | Turso, WAL                   | Yes   | `NORMAL`       |       10,399.2 |    2,002 |         5,838.6 |    3,409 |
| Fan-out write | Turso, MVCC                  | Yes   | `FULL`         |          892.4 |    2,643 |         1,637.6 |   18,815 |
| Fan-out write | Turso, MVCC                  | Yes   | `NORMAL`       |        6,650.8 |      156 |        24,726.2 |      616 |
| Retag         | PostgreSQL, `tokio-postgres` | Yes   | Server default |        1,489.4 |    1,914 |         8,069.4 |    2,401 |
| Retag         | PostgreSQL, SQLx             | Yes   | Server default |        1,486.4 |    1,390 |         8,075.6 |    2,385 |
| Retag         | SQLite, `rusqlite`           | No    | `FULL`         |        1,436.6 |    2,465 |         1,409.6 |    3,885 |
| Retag         | SQLite, `rusqlite`           | No    | `NORMAL`       |        8,965.8 |      125 |        23,759.0 |      284 |
| Retag         | SQLite, SQLx                 | Yes   | `FULL`         |        1,276.2 |    2,545 |         1,135.4 |    7,275 |
| Retag         | SQLite, SQLx                 | Yes   | `NORMAL`       |        5,376.0 |    1,482 |         5,634.8 |    6,595 |
| Retag         | Turso, WAL                   | Yes   | `FULL`         |          848.4 |    3,167 |         1,107.0 |    3,571 |
| Retag         | Turso, WAL                   | Yes   | `NORMAL`       |       14,390.8 |      160 |         6,938.8 |    1,524 |
| Retag         | Turso, MVCC                  | Yes   | `FULL`         |          869.6 |    2,619 |         1,613.8 |   18,335 |
| Retag         | Turso, MVCC                  | Yes   | `NORMAL`       |          687.2 |   21,631 |           411.6 |   46,239 |
| Indexed read  | PostgreSQL, `tokio-postgres` | Yes   | Server default |       17,248.0 |      145 |        60,519.4 |      268 |
| Indexed read  | PostgreSQL, SQLx             | Yes   | Server default |       19,436.4 |      136 |        59,250.2 |      276 |
| Indexed read  | SQLite, `rusqlite`           | No    | `FULL`         |      160,063.6 |        9 |        52,040.0 |      304 |
| Indexed read  | SQLite, `rusqlite`           | No    | `NORMAL`       |      153,598.4 |        8 |        51,534.4 |      316 |
| Indexed read  | SQLite, SQLx                 | Yes   | `FULL`         |       23,317.4 |       94 |        12,809.4 |    1,041 |
| Indexed read  | SQLite, SQLx                 | Yes   | `NORMAL`       |       24,326.4 |       88 |        13,144.0 |    1,040 |
| Indexed read  | Turso, WAL                   | Yes   | `FULL`         |       42,245.0 |       32 |       200,900.2 |       80 |
| Indexed read  | Turso, WAL                   | Yes   | `NORMAL`       |       50,085.2 |       34 |       224,039.2 |       79 |
| Indexed read  | Turso, MVCC                  | Yes   | `FULL`         |       25,506.4 |       52 |        75,152.6 |      229 |
| Indexed read  | Turso, MVCC                  | Yes   | `NORMAL`       |       26,013.6 |       59 |        76,220.0 |      226 |
| Join read     | PostgreSQL, `tokio-postgres` | Yes   | Server default |        8,139.0 |      276 |        27,810.2 |      517 |
| Join read     | PostgreSQL, SQLx             | Yes   | Server default |        8,747.8 |      215 |        23,922.4 |      633 |
| Join read     | SQLite, `rusqlite`           | No    | `FULL`         |       15,302.0 |       90 |         5,189.0 |    2,633 |
| Join read     | SQLite, `rusqlite`           | No    | `NORMAL`       |       15,743.0 |       93 |         5,300.4 |    2,651 |
| Join read     | SQLite, SQLx                 | Yes   | `FULL`         |       10,073.0 |      219 |         4,735.2 |    2,963 |
| Join read     | SQLite, SQLx                 | Yes   | `NORMAL`       |       10,944.0 |      190 |         4,663.4 |    2,941 |
| Join read     | Turso, WAL                   | Yes   | `FULL`         |        6,399.2 |      264 |        29,954.2 |      589 |
| Join read     | Turso, WAL                   | Yes   | `NORMAL`       |        7,759.0 |      187 |        36,464.8 |      407 |
| Join read     | Turso, MVCC                  | Yes   | `FULL`         |        4,351.4 |      351 |        15,789.6 |    1,094 |
| Join read     | Turso, MVCC                  | Yes   | `NORMAL`       |        4,495.8 |      334 |        15,953.6 |    1,101 |
| Balanced      | PostgreSQL, `tokio-postgres` | Yes   | Server default |        3,815.6 |    1,027 |        16,518.6 |    1,551 |
| Balanced      | PostgreSQL, SQLx             | Yes   | Server default |        3,850.8 |      922 |        17,049.4 |    1,498 |
| Balanced      | SQLite, `rusqlite`           | No    | `FULL`         |        3,536.4 |    1,596 |         3,173.0 |    5,235 |
| Balanced      | SQLite, `rusqlite`           | No    | `NORMAL`       |       16,499.4 |      121 |        20,514.8 |    6,743 |
| Balanced      | SQLite, SQLx                 | Yes   | `FULL`         |        3,082.2 |    1,722 |         2,631.8 |   57,247 |
| Balanced      | SQLite, SQLx                 | Yes   | `NORMAL`       |       10,129.6 |      235 |         9,742.8 |   13,175 |
| Balanced      | Turso, WAL                   | Yes   | `FULL`         |        1,941.6 |    2,221 |         2,499.0 |   33,823 |
| Balanced      | Turso, WAL                   | Yes   | `NORMAL`       |       18,069.2 |      176 |        13,480.4 |    8,167 |
| Balanced      | Turso, MVCC                  | Yes   | `FULL`         |        1,781.2 |    2,761 |         3,824.0 |   13,639 |
| Balanced      | Turso, MVCC                  | Yes   | `NORMAL`       |        3,814.2 |    2,321 |        11,146.6 |    5,951 |

Run configuration:

- 100 users, 10,000 todos, 1,000 tags, and 30,000 tag associations
- Three trials per reported mode, with rotated adapter, workload, and
  concurrency order
- 1-second warm-up followed by a 5-second measurement per case
- Durable profile: SQLite and both Turso journal modes `synchronous=FULL`
- Write-optimized profile: SQLite and both Turso journal modes
  `synchronous=NORMAL`
- Turso WAL and experimental MVCC journal modes, with MVCC writes using
  `BEGIN CONCURRENT` and retrying optimistic conflicts
- Normal PostgreSQL server durability in both profiles
- Fresh fixture for every case, including physical recreation of each embedded
  database and its journal sidecars
- PostgreSQL 17.10, SQLite 3.51.3, Turso 0.7.2, SQLx 0.9.0,
  `tokio-postgres` 0.7.18, and `rusqlite` 0.39.0
- Rust 1.97.1 release build on Linux 6.12.74, using an Intel Core i9-12900H
  with 14 logical CPUs available
- Zero failed measured or warm-up operations across all 360 published cases

Changing the synchronous mode primarily affects writes. SQLite and Turso WAL
`NORMAL` greatly increased write throughput while retaining their WAL syncs at
checkpoints and WAL restarts. Read-only results remained close across those
modes.

Turso MVCC is a different tradeoff. At eight clients with `NORMAL`, its fan-out
insert workload reached 24,726.2 ops/s, compared with 5,838.6 ops/s for Turso
WAL. Toggle and retag performance regressed sharply, and MVCC reads were slower
than WAL. With `FULL`, MVCC improved multi-client write throughput but increased
tail latency. Turso documents MVCC and `BEGIN CONCURRENT` as
[experimental concurrent-write features](https://docs.turso.tech/tursodb/concurrent-writes),
so these results should not be treated as production-readiness evidence.

Part of the third trial encountered a transient host-wide fsync slowdown that
reduced write throughput for PostgreSQL, SQLite, and Turso while leaving reads
normal. Per-metric medians reduce that outlier's effect, but durable-write
results remain sensitive to the storage device and system state. Treat this as
a reproducible local comparison, not a universal database ranking.

## Quick start

PostgreSQL server tools must be installed. The helper discovers them through
`pg_config`. Start the isolated local server, which listens only on a Unix
socket under `var`:

```console
scripts/postgres.sh start
```

Build and run the default balanced workload:

```console
cargo run --release
```

The default run creates 100 users, 10,000 todos, 1,000 tags, and 30,000 tag
associations for each adapter. It measures concurrency levels 1 and 8, with a
2-second warm-up and 10-second measurement period.

Stop PostgreSQL when it is no longer needed:

```console
scripts/postgres.sh stop
```

## Workloads

- `basic-write`: Toggle an existing todo.
- `fanout-write`: Insert a todo and three tag associations in one transaction.
- `retag`: Delete and replace three tag associations in one transaction.
- `indexed-read`: List the latest incomplete todos for a user.
- `join-read`: List incomplete todos matching both of two tags.
- `balanced`: 40 percent indexed reads, 20 percent join reads, 20 percent
  toggles, 15 percent fan-out inserts, and 5 percent retags.

Run every workload against every adapter:

```console
cargo run --release -- \
  --backends all \
  --workloads all \
  --concurrency 1,4,16,64 \
  --warmup-seconds 10 \
  --duration-seconds 60
```

Run only the two PostgreSQL drivers:

```console
cargo run --release -- \
  --backends postgres-native,postgres-sqlx \
  --workloads balanced \
  --concurrency 1,8,32
```

Valid backend names are `postgres-native`, `postgres-sqlx`, `sqlite-native`,
`sqlite-sqlx`, `turso`, and `turso-mvcc`.

## Larger fixture

This creates the medium fixture discussed in the benchmark design: 10,000 users,
1,000,000 todos, 100,000 tags, and 3,000,000 tag associations per adapter.

```console
cargo run --release -- \
  --users 10000 \
  --todos-per-user 100 \
  --tags-per-user 10 \
  --tags-per-todo 3 \
  --workloads balanced \
  --concurrency 1,8,32 \
  --warmup-seconds 30 \
  --duration-seconds 180
```

Fixture loading is not timed. By default, a fresh fixture is loaded before every
workload and concurrency case. This prevents faster write cases from leaving
larger datasets for later measurements. Loading the medium fixture for a full
matrix can take a long time, especially through row-oriented driver APIs. Pass
`--reuse-fixture` for faster exploratory runs where that isolation is not
required. The selected behavior is recorded in the JSON report.

## Write configuration

The default `--durability write-optimized` profile configures SQLite as:

- WAL journal mode
- `synchronous=NORMAL`
- 30-second busy timeout
- 128-entry statement cache
- Foreign-key enforcement enabled

It also configures both Turso variants with `synchronous=NORMAL`. The `turso`
backend uses the default WAL journal, while `turso-mvcc` applies
`PRAGMA journal_mode = 'mvcc'` after loading the fixture. MVCC write operations
use `BEGIN CONCURRENT`, roll back optimistic conflicts, and retry them inside
the measured operation. Both SQLite adapters use the same settings. All
adapters use persistent worker connections, prepared or cached statements,
explicit transactions for fan-out writes, and batched tag-association writes.

Use `--durability durable` to select `synchronous=FULL` for both SQLite
adapters and both Turso journal modes. PostgreSQL retains its normal durability
settings in both profiles. In WAL mode, SQLite and Turso `NORMAL` skip an fsync
for each commit but retain fsyncs at checkpoints and WAL restarts.

The profile is recorded in the JSON report. Do not compare results from
different durability profiles as if they represented the same guarantees.

## Output and storage

The terminal output reports throughput, latency percentiles in microseconds, and
failed operation counts. Detailed output is written to `var/results.json` by
default. Use `--output` to choose another path.

The generated layout is:

```text
var/
├── postgres/          PostgreSQL data directory
├── postgres-socket/   PostgreSQL Unix socket directory
├── postgres.log
├── sqlite-native.db
├── sqlite-sqlx.db
├── turso.db
├── turso-mvcc.db
├── turso-mvcc.db-log
└── results.json
```

`var` is ignored by Git. By default, every benchmark case resets the PostgreSQL
schema or physically recreates the selected SQLite or Turso database and its
journal sidecars, then loads a fresh fixture. PostgreSQL uses port 55432 by
default. Override it consistently:

```console
BENCH_PG_PORT=55433 scripts/postgres.sh start
cargo run --release -- --postgres-port 55433
```

## Benchmark interpretation

PostgreSQL runs as a local server while SQLite and Turso run in-process.
This is intentional, but it means the results include PostgreSQL protocol and
process boundary costs. The native and SQLx variants for a given database use
the same schema, transaction boundaries, storage configuration, and query
semantics.

Each client owns a connection. Connection creation and fixture loading are
outside the measured interval. Latency is measured around the database operation
only. Failed operations are excluded from throughput and latency percentiles,
then reported separately with the first error retained in the JSON output.
