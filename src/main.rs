mod backend;
mod benchmark;
mod model;
mod schema;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use anyhow::{Context, Result};
use backend::Backend;
use benchmark::{Report, run_case};
use clap::Parser;
use model::{Durability, FixtureConfig, parse_backends, parse_concurrency, parse_workloads};

#[derive(Debug, Parser)]
#[command(version, about = "Local PostgreSQL, SQLite, Turso, and SQLx benchmark")]
struct Cli {
    /// Comma-separated backend names, or "all".
    #[arg(long, default_value = "all")]
    backends: String,

    /// Comma-separated workload names, or "all".
    #[arg(long, default_value = "balanced")]
    workloads: String,

    /// Comma-separated client concurrency levels.
    #[arg(long, default_value = "1,8")]
    concurrency: String,

    #[arg(long, default_value_t = 100)]
    users: u64,

    #[arg(long, default_value_t = 100)]
    todos_per_user: u64,

    #[arg(long, default_value_t = 10)]
    tags_per_user: u64,

    #[arg(long, default_value_t = 3)]
    tags_per_todo: u64,

    #[arg(long, default_value_t = 2)]
    warmup_seconds: u64,

    #[arg(long, default_value_t = 10)]
    duration_seconds: u64,

    /// Reuse mutated data across cases instead of loading a fresh fixture.
    #[arg(long)]
    reuse_fixture: bool,

    #[arg(long, value_enum, default_value_t = Durability::WriteOptimized)]
    durability: Durability,

    #[arg(long, default_value = "var")]
    var_dir: PathBuf,

    #[arg(long, default_value_t = 55432)]
    postgres_port: u16,

    #[arg(long, default_value = "var/results.json")]
    output: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let backends = parse_backends(&cli.backends)?;
    let workloads = parse_workloads(&cli.workloads)?;
    let concurrency = parse_concurrency(&cli.concurrency)?;
    if cli.duration_seconds == 0 {
        anyhow::bail!("duration-seconds must be greater than zero");
    }

    let fixture = FixtureConfig {
        users: cli.users,
        todos_per_user: cli.todos_per_user,
        tags_per_user: cli.tags_per_user,
        tags_per_todo: cli.tags_per_todo,
    };
    fixture.validate()?;
    std::fs::create_dir_all(&cli.var_dir)
        .with_context(|| format!("create {}", cli.var_dir.display()))?;

    let mut report = Report::new(
        cli.durability,
        fixture.clone(),
        cli.warmup_seconds,
        cli.duration_seconds,
        !cli.reuse_fixture,
    );
    print_header();

    for backend_kind in backends {
        let mut backend = Backend::open(
            backend_kind,
            &cli.var_dir,
            cli.postgres_port,
            cli.durability,
        )
        .await
        .with_context(|| format!("open {backend_kind}"))?;
        let shared_next_todo_id = Arc::new(AtomicU64::new(fixture.seed_todo_count() + 1));
        let mut seeded = false;
        for workload in &workloads {
            for &clients in &concurrency {
                if !seeded || !cli.reuse_fixture {
                    eprintln!(
                        "Seeding {backend_kind} for {workload}/{clients} with {} users, {} todos, and {} tags...",
                        fixture.users,
                        fixture.seed_todo_count(),
                        fixture.users * fixture.tags_per_user
                    );
                    backend.reset_and_seed(&fixture).await.with_context(|| {
                        if backend_kind.is_postgres() {
                            format!(
                                "seed {backend_kind}; start the local server with scripts/postgres.sh start"
                            )
                        } else {
                            format!("seed {backend_kind}")
                        }
                    })?;
                    seeded = true;
                }
                let next_todo_id = if cli.reuse_fixture {
                    Arc::clone(&shared_next_todo_id)
                } else {
                    Arc::new(AtomicU64::new(fixture.seed_todo_count() + 1))
                };
                let result = run_case(
                    &backend,
                    &fixture,
                    *workload,
                    clients,
                    Duration::from_secs(cli.warmup_seconds),
                    Duration::from_secs(cli.duration_seconds),
                    next_todo_id,
                )
                .await
                .with_context(|| {
                    format!("run {backend_kind}/{workload} at concurrency {clients}")
                })?;
                print_result(&result);
                report.results.push(result);
            }
        }
    }

    if let Some(parent) = cli.output.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(&report)?;
    std::fs::write(&cli.output, json).with_context(|| format!("write {}", cli.output.display()))?;
    eprintln!("Results written to {}", cli.output.display());
    Ok(())
}

fn print_header() {
    println!(
        "{:<17} {:<14} {:>7} {:>11} {:>10} {:>10} {:>10} {:>8}",
        "backend", "workload", "clients", "ops/s", "p50 us", "p95 us", "p99 us", "errors"
    );
}

fn print_result(result: &benchmark::BenchmarkResult) {
    println!(
        "{:<17} {:<14} {:>7} {:>11.1} {:>10} {:>10} {:>10} {:>8}",
        result.backend.to_string(),
        result.workload.to_string(),
        result.concurrency,
        result.operations_per_second,
        result.latency_us.p50,
        result.latency_us.p95,
        result.latency_us.p99,
        result.failed_operations,
    );
    if let Some(error) = &result.first_error {
        eprintln!(
            "First error for {}/{} at concurrency {}: {}",
            result.backend, result.workload, result.concurrency, error
        );
    }
}
