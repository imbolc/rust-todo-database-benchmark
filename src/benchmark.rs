use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use hdrhistogram::Histogram;
use serde::Serialize;
use tokio::sync::Barrier;

use crate::backend::Backend;
use crate::model::{BackendKind, Durability, FixtureConfig, Operation, Workload};

const MAX_RECORDED_LATENCY_US: u64 = 60_000_000;

#[derive(Debug, Serialize)]
pub struct BenchmarkResult {
    pub backend: BackendKind,
    pub database: String,
    pub driver: String,
    pub storage: String,
    pub workload: Workload,
    pub concurrency: usize,
    pub duration_seconds: f64,
    pub successful_operations: u64,
    pub failed_operations: u64,
    pub warmup_failures: u64,
    pub operations_per_second: f64,
    pub latency_us: Latencies,
    pub first_error: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct Latencies {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub generated_at_unix_seconds: u64,
    pub durability: Durability,
    pub fixture: FixtureConfig,
    pub warmup_seconds: u64,
    pub duration_seconds: u64,
    pub reset_between_cases: bool,
    pub results: Vec<BenchmarkResult>,
}

impl Report {
    pub fn new(
        durability: Durability,
        fixture: FixtureConfig,
        warmup_seconds: u64,
        duration_seconds: u64,
        reset_between_cases: bool,
    ) -> Self {
        Self {
            generated_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            durability,
            fixture,
            warmup_seconds,
            duration_seconds,
            reset_between_cases,
            results: Vec::new(),
        }
    }
}

struct WorkerStats {
    histogram: Histogram<u64>,
    successes: u64,
    failures: u64,
    warmup_failures: u64,
    first_error: Option<String>,
}

impl WorkerStats {
    fn new() -> Self {
        Self {
            histogram: Histogram::new_with_max(MAX_RECORDED_LATENCY_US, 3)
                .expect("valid histogram configuration"),
            successes: 0,
            failures: 0,
            warmup_failures: 0,
            first_error: None,
        }
    }
}

pub async fn run_case(
    backend: &Backend,
    fixture: &FixtureConfig,
    workload: Workload,
    concurrency: usize,
    warmup: Duration,
    duration: Duration,
    next_todo_id: Arc<AtomicU64>,
) -> Result<BenchmarkResult> {
    let mut workers = Vec::with_capacity(concurrency);
    for worker_index in 0..concurrency {
        workers.push(
            backend
                .connect_worker()
                .await
                .with_context(|| format!("connect benchmark worker {worker_index}"))?,
        );
    }

    let barrier = Arc::new(Barrier::new(concurrency + 1));
    let sequence = Arc::new(AtomicU64::new(0));
    let start = Instant::now() + Duration::from_millis(100);
    let measure_start = start + warmup;
    let end = measure_start + duration;
    let mut tasks = Vec::with_capacity(concurrency);

    for mut worker in workers {
        let barrier = Arc::clone(&barrier);
        let sequence = Arc::clone(&sequence);
        let next_todo_id = Arc::clone(&next_todo_id);
        let fixture = fixture.clone();
        tasks.push(tokio::spawn(async move {
            let mut stats = WorkerStats::new();
            barrier.wait().await;
            tokio::time::sleep_until(tokio::time::Instant::from_std(start)).await;

            loop {
                let now = Instant::now();
                if now >= end {
                    break;
                }
                let measured = now >= measure_start;
                let sequence = sequence.fetch_add(1, Ordering::Relaxed);
                let operation = operation_for(workload, sequence, &fixture, next_todo_id.as_ref());
                let operation_start = Instant::now();
                let result = worker.execute(&operation).await;
                let elapsed = operation_start.elapsed();

                match (measured, result) {
                    (true, Ok(())) => {
                        stats.successes += 1;
                        let micros = elapsed
                            .as_micros()
                            .clamp(1, MAX_RECORDED_LATENCY_US as u128)
                            as u64;
                        stats
                            .histogram
                            .record(micros)
                            .expect("latency is clamped to histogram range");
                    }
                    (true, Err(error)) => {
                        stats.failures += 1;
                        if stats.first_error.is_none() {
                            stats.first_error = Some(format!("{error:#}"));
                        }
                    }
                    (false, Err(error)) => {
                        stats.warmup_failures += 1;
                        if stats.first_error.is_none() {
                            stats.first_error = Some(format!("{error:#}"));
                        }
                    }
                    (false, Ok(())) => {}
                }
            }
            stats
        }));
    }

    barrier.wait().await;
    let mut merged = WorkerStats::new();
    for task in tasks {
        let stats = task.await.context("benchmark worker panicked")?;
        merged.histogram.add(&stats.histogram)?;
        merged.successes += stats.successes;
        merged.failures += stats.failures;
        merged.warmup_failures += stats.warmup_failures;
        if merged.first_error.is_none() {
            merged.first_error = stats.first_error;
        }
    }

    let latency_us = if merged.histogram.is_empty() {
        Latencies::default()
    } else {
        Latencies {
            p50: merged.histogram.value_at_quantile(0.50),
            p95: merged.histogram.value_at_quantile(0.95),
            p99: merged.histogram.value_at_quantile(0.99),
            max: merged.histogram.max(),
        }
    };

    Ok(BenchmarkResult {
        backend: backend.kind(),
        database: backend.kind().database_label().to_owned(),
        driver: backend.kind().driver_label().to_owned(),
        storage: backend.storage(),
        workload,
        concurrency,
        duration_seconds: duration.as_secs_f64(),
        successful_operations: merged.successes,
        failed_operations: merged.failures,
        warmup_failures: merged.warmup_failures,
        operations_per_second: merged.successes as f64 / duration.as_secs_f64(),
        latency_us,
        first_error: merged.first_error,
    })
}

fn operation_for(
    workload: Workload,
    sequence: u64,
    fixture: &FixtureConfig,
    next_todo_id: &AtomicU64,
) -> Operation {
    match workload {
        Workload::BasicWrite => toggle_operation(sequence, fixture),
        Workload::FanoutWrite => insert_operation(sequence, fixture, next_todo_id),
        Workload::Retag => retag_operation(sequence, fixture),
        Workload::IndexedRead => Operation::ListTodos {
            user_id: fixture.user_id(sequence),
        },
        Workload::JoinRead => join_operation(sequence, fixture),
        Workload::Balanced => match sequence % 100 {
            0..=39 => Operation::ListTodos {
                user_id: fixture.user_id(sequence),
            },
            40..=59 => join_operation(sequence, fixture),
            60..=79 => toggle_operation(sequence, fixture),
            80..=94 => insert_operation(sequence, fixture, next_todo_id),
            _ => retag_operation(sequence, fixture),
        },
    }
}

fn toggle_operation(sequence: u64, fixture: &FixtureConfig) -> Operation {
    Operation::Toggle {
        todo_id: fixture.todo_id(sequence),
        completed: sequence.is_multiple_of(2),
        updated_at: 2_000_000_000 + (sequence % 1_000_000_000) as i64,
    }
}

fn insert_operation(sequence: u64, fixture: &FixtureConfig, next_todo_id: &AtomicU64) -> Operation {
    let user_id = fixture.user_id(sequence);
    let todo_id = next_todo_id.fetch_add(1, Ordering::Relaxed) as i64;
    let first_tag = (sequence / fixture.users) % fixture.tags_per_user;
    Operation::InsertTagged {
        todo_id,
        user_id,
        title: format!("Benchmarked todo {todo_id}"),
        timestamp: 2_000_000_000 + (sequence % 1_000_000_000) as i64,
        tag_ids: (0..3)
            .map(|offset| fixture.tag_id(user_id, first_tag + offset))
            .collect(),
    }
}

fn retag_operation(sequence: u64, fixture: &FixtureConfig) -> Operation {
    let user_id = fixture.user_id(sequence);
    let first_tag = (sequence / fixture.users + 3) % fixture.tags_per_user;
    Operation::Retag {
        todo_id: fixture.todo_id(sequence),
        user_id,
        tag_ids: (0..3)
            .map(|offset| fixture.tag_id(user_id, first_tag + offset))
            .collect(),
    }
}

fn join_operation(sequence: u64, fixture: &FixtureConfig) -> Operation {
    let (user_id, tag_a, tag_b) = fixture.tag_pair(sequence);
    Operation::ListByTags {
        user_id,
        tag_a,
        tag_b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> FixtureConfig {
        FixtureConfig {
            users: 10,
            todos_per_user: 20,
            tags_per_user: 8,
            tags_per_todo: 3,
        }
    }

    #[test]
    fn generated_fixture_ids_remain_in_range() {
        let fixture = fixture();
        for sequence in 0..1_000 {
            assert!((1..=10).contains(&fixture.user_id(sequence)));
            assert!((1..=200).contains(&fixture.todo_id(sequence)));
            let (user_id, tag_a, tag_b) = fixture.tag_pair(sequence);
            let first = (user_id - 1) * 8 + 1;
            assert!((first..first + 8).contains(&tag_a));
            assert!((first..first + 8).contains(&tag_b));
        }
    }

    #[test]
    fn balanced_mix_has_expected_operation_counts() {
        let fixture = fixture();
        let next_id = AtomicU64::new(201);
        let mut counts = [0_u64; 5];
        for sequence in 0..100 {
            let index = match operation_for(Workload::Balanced, sequence, &fixture, &next_id) {
                Operation::ListTodos { .. } => 0,
                Operation::ListByTags { .. } => 1,
                Operation::Toggle { .. } => 2,
                Operation::InsertTagged { .. } => 3,
                Operation::Retag { .. } => 4,
            };
            counts[index] += 1;
        }
        assert_eq!(counts, [40, 20, 20, 15, 5]);
    }
}
