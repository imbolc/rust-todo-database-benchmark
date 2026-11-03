# PostgreSQL vs SQLite vs Turso vs TypeDB

This project benchmarks four local databases through six Rust adapters and
seven runnable configurations:

| Database   | Adapter                            | Async |
| ---------- | ---------------------------------- | ----- |
| PostgreSQL | `tokio-postgres`                   | Yes   |
| PostgreSQL | SQLx                               | Yes   |
| SQLite     | `rusqlite`                         | No    |
| SQLite     | SQLx                               | Yes   |
| Turso      | Native `turso` crate, WAL and MVCC | Yes   |
| TypeDB     | Native `typedb-driver` crate       | Yes   |

`Async` describes the driver API used by the benchmark. SQLx exposes an async
SQLite API by dispatching SQLite operations to a worker thread; SQLite itself
remains synchronous.

The fixture models users, todos, user-owned tags, and a many-to-many `todo_tags`
relation. SQLite and Turso files, PostgreSQL data, and benchmark reports are
kept in `./var`. TypeDB stores its dedicated benchmark database in the local
server's configured data directory.

## Latest local results

These results are the per-metric medians of three release trials run on
2026-11-03. Every trial covered all six workloads at both concurrency levels.
SQLite and Turso were tested with both durability profiles. PostgreSQL and
TypeDB appear once because the profiles do not change their server settings.

The six raw JSON reports, console logs, commands, host information, and
aggregated medians are saved under `var/benchmarks-2026-11-03/`. The JSON
reports also contain p50, p95, maximum latency, and operation counts.

### Selected async configurations

| Workload      | Adapter                 | Async | Mode            | 1 client ops/s | p99 (us) | 8 clients ops/s | p99 (us) |
| ------------- | ----------------------- | ----- | --------------- | -------------: | -------: | --------------: | -------: |
| Basic write   | PostgreSQL, SQLx        | Yes   | Server settings |        5,155.0 |      260 |        22,176.6 |      492 |
| Basic write   | SQLite, SQLx            | Yes   | `NORMAL`        |       81,510.8 |       12 |        78,869.0 |    1,069 |
| Basic write   | Turso, WAL              | Yes   | `NORMAL`        |       87,737.8 |       17 |        74,531.2 |       23 |
| Basic write   | Turso, MVCC             | Yes   | `NORMAL`        |          561.8 |    5,947 |         1,924.2 |   17,551 |
| Basic write   | TypeDB, `typedb-driver` | Yes   | Server settings |          943.8 |    1,394 |         4,343.0 |   11,559 |
| Fan-out write | PostgreSQL, SQLx        | Yes   | Server settings |        3,977.0 |      323 |        20,888.0 |      534 |
| Fan-out write | SQLite, SQLx            | Yes   | `NORMAL`        |       16,931.0 |      459 |        15,060.2 |    8,239 |
| Fan-out write | Turso, WAL              | Yes   | `NORMAL`        |       16,383.4 |      756 |        14,602.6 |    1,374 |
| Fan-out write | Turso, MVCC             | Yes   | `NORMAL`        |        9,836.4 |       78 |        44,303.6 |      248 |
| Fan-out write | TypeDB, `typedb-driver` | Yes   | Server settings |          511.2 |    2,517 |         3,541.4 |   11,495 |
| Retag         | PostgreSQL, SQLx        | Yes   | Server settings |        4,041.4 |      329 |        21,133.6 |      527 |
| Retag         | SQLite, SQLx            | Yes   | `NORMAL`        |       25,586.6 |       62 |        23,450.4 |    1,284 |
| Retag         | Turso, WAL              | Yes   | `NORMAL`        |       23,695.8 |       74 |        19,961.4 |      150 |
| Retag         | Turso, MVCC             | Yes   | `NORMAL`        |          200.4 |   21,135 |           644.4 |   32,751 |
| Retag         | TypeDB, `typedb-driver` | Yes   | Server settings |          474.4 |    2,285 |         3,561.0 |   11,511 |
| Indexed read  | PostgreSQL, SQLx        | Yes   | Server settings |       49,245.4 |       30 |       267,683.6 |       47 |
| Indexed read  | SQLite, SQLx            | Yes   | `NORMAL`        |       67,362.4 |       32 |        34,841.6 |      346 |
| Indexed read  | Turso, WAL              | Yes   | `NORMAL`        |       80,520.2 |       13 |       588,370.8 |       17 |
| Indexed read  | Turso, MVCC             | Yes   | `NORMAL`        |       49,735.0 |       21 |       152,974.4 |       75 |
| Indexed read  | TypeDB, `typedb-driver` | Yes   | Server settings |        1,097.6 |    1,412 |         6,587.8 |    1,627 |
| Join read     | PostgreSQL, SQLx        | Yes   | Server settings |       18,382.6 |       62 |        96,838.4 |      115 |
| Join read     | SQLite, SQLx            | Yes   | `NORMAL`        |       23,176.2 |       56 |        12,805.8 |    1,003 |
| Join read     | Turso, WAL              | Yes   | `NORMAL`        |       12,231.8 |       93 |        88,090.0 |      132 |
| Join read     | Turso, MVCC             | Yes   | `NORMAL`        |        8,885.4 |      137 |        36,584.2 |      318 |
| Join read     | TypeDB, `typedb-driver` | Yes   | Server settings |        1,252.2 |    1,218 |         7,792.2 |    1,349 |
| Balanced      | PostgreSQL, SQLx        | Yes   | Server settings |        9,199.4 |      288 |        45,200.2 |      447 |
| Balanced      | SQLite, SQLx            | Yes   | `NORMAL`        |       35,886.8 |       57 |        22,865.4 |    3,549 |
| Balanced      | Turso, WAL              | Yes   | `NORMAL`        |       28,159.6 |      109 |        31,510.6 |    3,085 |
| Balanced      | Turso, MVCC             | Yes   | `NORMAL`        |        6,841.6 |    1,174 |        20,861.8 |    3,331 |
| Balanced      | TypeDB, `typedb-driver` | Yes   | Server settings |          839.4 |    2,167 |         5,256.2 |    2,799 |

### Detailed results

| Workload      | Adapter                      | Async | Mode            | 1 client ops/s | p99 (us) | 8 clients ops/s | p99 (us) |
| ------------- | ---------------------------- | ----- | --------------- | -------------: | -------: | --------------: | -------: |
| Basic write   | PostgreSQL, `tokio-postgres` | Yes   | Server settings |        5,203.8 |      250 |        22,304.8 |      438 |
| Basic write   | PostgreSQL, SQLx             | Yes   | Server settings |        5,155.0 |      260 |        22,176.6 |      492 |
| Basic write   | SQLite, `rusqlite`           | No    | `FULL`          |        5,067.6 |      424 |         5,078.8 |      430 |
| Basic write   | SQLite, `rusqlite`           | No    | `NORMAL`        |      115,015.4 |        7 |       128,753.6 |      585 |
| Basic write   | SQLite, SQLx                 | Yes   | `FULL`          |        4,791.6 |      439 |         4,846.4 |      448 |
| Basic write   | SQLite, SQLx                 | Yes   | `NORMAL`        |       81,510.8 |       12 |        78,869.0 |    1,069 |
| Basic write   | Turso, WAL                   | Yes   | `FULL`          |        2,501.2 |      520 |         2,318.0 |    1,220 |
| Basic write   | Turso, WAL                   | Yes   | `NORMAL`        |       87,737.8 |       17 |        74,531.2 |       23 |
| Basic write   | Turso, MVCC                  | Yes   | `FULL`          |        1,681.6 |    5,251 |         2,054.6 |   16,167 |
| Basic write   | Turso, MVCC                  | Yes   | `NORMAL`        |          561.8 |    5,947 |         1,924.2 |   17,551 |
| Basic write   | TypeDB, `typedb-driver`      | Yes   | Server settings |          943.8 |    1,394 |         4,343.0 |   11,559 |
| Fan-out write | PostgreSQL, `tokio-postgres` | Yes   | Server settings |        3,994.0 |      339 |        20,980.4 |      530 |
| Fan-out write | PostgreSQL, SQLx             | Yes   | Server settings |        3,977.0 |      323 |        20,888.0 |      534 |
| Fan-out write | SQLite, `rusqlite`           | No    | `FULL`          |        4,564.8 |      578 |         4,543.8 |      591 |
| Fan-out write | SQLite, `rusqlite`           | No    | `NORMAL`        |       23,500.6 |      529 |        31,297.2 |    2,775 |
| Fan-out write | SQLite, SQLx                 | Yes   | `FULL`          |        4,060.0 |      622 |         4,008.4 |    1,174 |
| Fan-out write | SQLite, SQLx                 | Yes   | `NORMAL`        |       16,931.0 |      459 |        15,060.2 |    8,239 |
| Fan-out write | Turso, WAL                   | Yes   | `FULL`          |        2,171.6 |    1,287 |         2,013.0 |    1,527 |
| Fan-out write | Turso, WAL                   | Yes   | `NORMAL`        |       16,383.4 |      756 |        14,602.6 |    1,374 |
| Fan-out write | Turso, MVCC                  | Yes   | `FULL`          |        2,120.8 |      723 |         2,429.8 |   13,311 |
| Fan-out write | Turso, MVCC                  | Yes   | `NORMAL`        |        9,836.4 |       78 |        44,303.6 |      248 |
| Fan-out write | TypeDB, `typedb-driver`      | Yes   | Server settings |          511.2 |    2,517 |         3,541.4 |   11,495 |
| Retag         | PostgreSQL, `tokio-postgres` | Yes   | Server settings |        4,031.6 |      316 |        21,323.4 |      469 |
| Retag         | PostgreSQL, SQLx             | Yes   | Server settings |        4,041.4 |      329 |        21,133.6 |      527 |
| Retag         | SQLite, `rusqlite`           | No    | `FULL`          |        4,743.4 |      443 |         4,732.8 |      447 |
| Retag         | SQLite, `rusqlite`           | No    | `NORMAL`        |       50,820.4 |       21 |        47,660.4 |    1,092 |
| Retag         | SQLite, SQLx                 | Yes   | `FULL`          |        4,194.8 |      487 |         4,153.2 |      485 |
| Retag         | SQLite, SQLx                 | Yes   | `NORMAL`        |       25,586.6 |       62 |        23,450.4 |    1,284 |
| Retag         | Turso, WAL                   | Yes   | `FULL`          |        2,225.4 |    1,161 |         2,046.4 |    1,450 |
| Retag         | Turso, WAL                   | Yes   | `NORMAL`        |       23,695.8 |       74 |        19,961.4 |      150 |
| Retag         | Turso, MVCC                  | Yes   | `FULL`          |        1,610.4 |    5,463 |         1,755.2 |   26,575 |
| Retag         | Turso, MVCC                  | Yes   | `NORMAL`        |          200.4 |   21,135 |           644.4 |   32,751 |
| Retag         | TypeDB, `typedb-driver`      | Yes   | Server settings |          474.4 |    2,285 |         3,561.0 |   11,511 |
| Indexed read  | PostgreSQL, `tokio-postgres` | Yes   | Server settings |       51,732.6 |       27 |       289,473.0 |       39 |
| Indexed read  | PostgreSQL, SQLx             | Yes   | Server settings |       49,245.4 |       30 |       267,683.6 |       47 |
| Indexed read  | SQLite, `rusqlite`           | No    | `FULL`          |      328,516.8 |        3 |       136,444.2 |      119 |
| Indexed read  | SQLite, `rusqlite`           | No    | `NORMAL`        |      327,939.8 |        3 |       122,009.2 |      122 |
| Indexed read  | SQLite, SQLx                 | Yes   | `FULL`          |       70,032.0 |       31 |        40,873.0 |      340 |
| Indexed read  | SQLite, SQLx                 | Yes   | `NORMAL`        |       67,362.4 |       32 |        34,841.6 |      346 |
| Indexed read  | Turso, WAL                   | Yes   | `FULL`          |       80,757.6 |       12 |       582,974.4 |       18 |
| Indexed read  | Turso, WAL                   | Yes   | `NORMAL`        |       80,520.2 |       13 |       588,370.8 |       17 |
| Indexed read  | Turso, MVCC                  | Yes   | `FULL`          |       49,750.4 |       21 |       167,919.8 |       74 |
| Indexed read  | Turso, MVCC                  | Yes   | `NORMAL`        |       49,735.0 |       21 |       152,974.4 |       75 |
| Indexed read  | TypeDB, `typedb-driver`      | Yes   | Server settings |        1,097.6 |    1,412 |         6,587.8 |    1,627 |
| Join read     | PostgreSQL, `tokio-postgres` | Yes   | Server settings |       18,498.0 |       76 |       100,429.8 |      119 |
| Join read     | PostgreSQL, SQLx             | Yes   | Server settings |       18,382.6 |       62 |        96,838.4 |      115 |
| Join read     | SQLite, `rusqlite`           | No    | `FULL`          |       32,582.8 |       41 |        11,961.4 |    1,018 |
| Join read     | SQLite, `rusqlite`           | No    | `NORMAL`        |       32,545.6 |       33 |        13,060.6 |      962 |
| Join read     | SQLite, SQLx                 | Yes   | `FULL`          |       23,140.8 |       58 |         9,789.8 |    1,064 |
| Join read     | SQLite, SQLx                 | Yes   | `NORMAL`        |       23,176.2 |       56 |        12,805.8 |    1,003 |
| Join read     | Turso, WAL                   | Yes   | `FULL`          |       12,244.6 |       96 |        88,447.8 |      138 |
| Join read     | Turso, WAL                   | Yes   | `NORMAL`        |       12,231.8 |       93 |        88,090.0 |      132 |
| Join read     | Turso, MVCC                  | Yes   | `FULL`          |        9,039.4 |      143 |        35,475.8 |      321 |
| Join read     | Turso, MVCC                  | Yes   | `NORMAL`        |        8,885.4 |      137 |        36,584.2 |      318 |
| Join read     | TypeDB, `typedb-driver`      | Yes   | Server settings |        1,252.2 |    1,218 |         7,792.2 |    1,349 |
| Balanced      | PostgreSQL, `tokio-postgres` | Yes   | Server settings |        9,180.6 |      287 |        44,456.8 |      457 |
| Balanced      | PostgreSQL, SQLx             | Yes   | Server settings |        9,199.4 |      288 |        45,200.2 |      447 |
| Balanced      | SQLite, `rusqlite`           | No    | `FULL`          |       11,013.4 |      422 |        10,684.6 |      444 |
| Balanced      | SQLite, `rusqlite`           | No    | `NORMAL`        |       59,799.0 |       40 |        45,085.0 |    3,125 |
| Balanced      | SQLite, SQLx                 | Yes   | `FULL`          |        9,400.4 |      436 |         9,143.2 |      477 |
| Balanced      | SQLite, SQLx                 | Yes   | `NORMAL`        |       35,886.8 |       57 |        22,865.4 |    3,549 |
| Balanced      | Turso, WAL                   | Yes   | `FULL`          |        5,082.6 |      545 |         4,594.4 |    1,078 |
| Balanced      | Turso, WAL                   | Yes   | `NORMAL`        |       28,159.6 |      109 |        31,510.6 |    3,085 |
| Balanced      | Turso, MVCC                  | Yes   | `FULL`          |        3,571.4 |    1,577 |         5,943.4 |    9,535 |
| Balanced      | Turso, MVCC                  | Yes   | `NORMAL`        |        6,841.6 |    1,174 |        20,861.8 |    3,331 |
| Balanced      | TypeDB, `typedb-driver`      | Yes   | Server settings |          839.4 |    2,167 |         5,256.2 |    2,799 |

Run configuration:

- 100 users, 10,000 todos, 1,000 tags, and 30,000 tag associations
- Three trials per reported mode, with rotated adapter, workload, and
  concurrency order
- 1-second warm-up followed by a 5-second measurement per case
- Durable profile: SQLite and both Turso journal modes `synchronous=FULL`
- Write-optimized profile: SQLite and both Turso journal modes
  `synchronous=NORMAL`
- Turso MVCC writes use `BEGIN CONCURRENT` and retry optimistic conflicts
  inside the measured operation, following the
  [concurrent-write transaction model](https://docs.turso.tech/tursodb/concurrent-writes)
- PostgreSQL and TypeDB retain their server durability settings
- Fresh fixture for every case: reset PostgreSQL tables, recreate the
  dedicated TypeDB database, and physically recreate embedded databases and
  their journal sidecars
- PostgreSQL 17.11, SQLite 3.51.3, Turso 0.7.2, SQLx 0.9.0,
  `tokio-postgres` 0.7.18, `rusqlite` 0.39.0, TypeDB 3.14.1, and
  `typedb-driver` 3.14.0
- Rust 1.99.1 release build on Linux 6.12.113+deb13-amd64, using an AMD
  Ryzen 9 9950X3D2 processor with 32 logical CPUs available
- Zero failed measured or warm-up operations across all 396 cases

TypeDB's balanced workload reached 839.4 ops/s at one client and
5,256.2 ops/s at eight clients. PostgreSQL through SQLx reached
9,199.4 and 45,200.2 ops/s respectively. These adapters include different
protocol and transaction costs, described under benchmark interpretation.

At eight clients with `NORMAL`, Turso MVCC reached
44,303.6 ops/s for fan-out inserts, compared with
14,602.6 ops/s for Turso WAL. MVCC's toggle and retag throughput was
lower than WAL's in the same profile, so the journal-mode tradeoff depends on
the workload. Compare durability modes separately: their sync guarantees
differ. These are local measurements for this fixture and implementation.

## Quick start

PostgreSQL server tools must be installed. The helper discovers them through
`pg_config`. Start the isolated local server, which listens only on a Unix
socket under `var`:

```console
scripts/postgres.sh start
```

TypeDB 3.14 must also be running locally. The default connection is
`localhost:1729`, with username `admin`, password `password`, and TLS disabled.
Set `TYPEDB_ADDRESS`, `TYPEDB_USERNAME`, or `TYPEDB_PASSWORD` to override these
defaults, or use the corresponding `--typedb-address`, `--typedb-username`, and
`--typedb-password` options. Use `--typedb-tls` for a server with TLS enabled.

Seeding deletes and recreates the dedicated `rust-todo-benchmark` TypeDB
database. Override its name with `--typedb-database` or `TYPEDB_DATABASE`, and
use a database that contains only benchmark data.

Build and run the default balanced workload:

```console
cargo run --release -p rust-todo-database-benchmark --
```

The default run creates 100 users, 10,000 todos, 1,000 tags, and 30,000 tag
associations for each adapter. It measures concurrency levels 1 and 8, with a
2-second warm-up and 10-second measurement period.

Stop PostgreSQL when it is no longer needed:

```console
scripts/postgres.sh stop
```

## Workloads

- `basic-write`: Toggle an existing todo
- `fanout-write`: Insert a todo and three tag associations in one transaction
- `retag`: Delete and replace three tag associations in one transaction
- `indexed-read`: List the latest incomplete todos for a user
- `join-read`: List incomplete todos matching both of two tags
- `balanced`: 40 percent indexed reads, 20 percent join reads, 20 percent
  toggles, 15 percent fan-out inserts, and 5 percent retags

Run one durable trial of every workload against every adapter using the
published timing and concurrency settings:

```console
cargo run --release -p rust-todo-database-benchmark -- \
  --backends all \
  --workloads all \
  --concurrency 1,8 \
  --warmup-seconds 1 \
  --duration-seconds 5 \
  --durability durable \
  --output var/results-durable.json
```

Run the embedded adapters with the write-optimized profile:

```console
cargo run --release -p rust-todo-database-benchmark -- \
  --backends sqlite-native,sqlite-sqlx,turso,turso-mvcc \
  --workloads all \
  --concurrency 1,8 \
  --warmup-seconds 1 \
  --duration-seconds 5 \
  --durability write-optimized \
  --output var/results-write-optimized.json
```

The published results repeat both commands three times with rotated adapter,
workload, and concurrency order. PostgreSQL and TypeDB need only one profile
because their server settings remain unchanged.

Run only the two PostgreSQL drivers:

```console
cargo run --release -p rust-todo-database-benchmark -- \
  --backends postgres-native,postgres-sqlx \
  --workloads balanced \
  --concurrency 1,8,32
```

Valid backend names are `postgres-native`, `postgres-sqlx`, `sqlite-native`,
`sqlite-sqlx`, `turso`, `turso-mvcc`, and `typedb`.

## Larger fixture

This creates the medium fixture discussed in the benchmark design: 10,000 users,
1,000,000 todos, 100,000 tags, and 3,000,000 tag associations per adapter.

```console
cargo run --release -p rust-todo-database-benchmark -- \
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
SQL adapters use persistent worker connections and prepared or cached
statements. TypeDB shares a native driver's connection pool across workers,
opens a transaction for each operation, and binds query parameters through
TypeQL `given` rows. Its schema represents todos and tags as relations to their
owning user, with separate todo-tag relations. All adapters use explicit
transactions for fan-out writes.

Use `--durability durable` to select `synchronous=FULL` for both SQLite
adapters and both Turso journal modes. PostgreSQL and TypeDB retain their server
durability settings in both profiles. In WAL mode, SQLite and Turso `NORMAL`
skip an fsync for each commit but retain fsyncs at checkpoints and WAL restarts.

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
schema, recreates the dedicated TypeDB database, or physically recreates the
selected SQLite or Turso database and its journal sidecars, then loads a fresh
fixture. PostgreSQL uses port 55432 by default. Override it consistently:

```console
BENCH_PG_PORT=55433 scripts/postgres.sh start
cargo run --release -p rust-todo-database-benchmark -- --postgres-port 55433
```

## Benchmark interpretation

PostgreSQL and TypeDB run as local servers, while SQLite and Turso run
in-process. Their results include protocol and process boundary costs.
PostgreSQL uses a Unix socket; TypeDB uses TCP. The native and SQLx variants for
a given database use the same schema, transaction boundaries, storage
configuration, and query semantics.

Each SQL client owns a connection. TypeDB clients share the driver's connection
pool and open independent transactions. Driver initialization, SQL connection
creation, and fixture loading are outside the measured interval. TypeDB
transaction creation, query streaming, and commit or close are inside it.
Latency is measured around the database operation only. Failed operations are
excluded from throughput and latency percentiles, then reported separately with
the first error retained in the JSON output.
