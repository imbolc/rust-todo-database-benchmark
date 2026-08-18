use std::fmt;

use anyhow::{Context, Result, anyhow, bail};
use clap::ValueEnum;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    PostgresNative,
    PostgresSqlx,
    SqliteNative,
    SqliteSqlx,
    Turso,
    TursoMvcc,
}

impl BackendKind {
    pub const ALL: [Self; 6] = [
        Self::PostgresNative,
        Self::PostgresSqlx,
        Self::SqliteNative,
        Self::SqliteSqlx,
        Self::Turso,
        Self::TursoMvcc,
    ];

    pub fn is_postgres(self) -> bool {
        matches!(self, Self::PostgresNative | Self::PostgresSqlx)
    }

    pub fn database_label(self) -> &'static str {
        match self {
            Self::PostgresNative | Self::PostgresSqlx => "postgres",
            Self::SqliteNative | Self::SqliteSqlx => "sqlite",
            Self::Turso | Self::TursoMvcc => "turso",
        }
    }

    pub fn driver_label(self) -> &'static str {
        match self {
            Self::PostgresNative => "tokio-postgres",
            Self::PostgresSqlx | Self::SqliteSqlx => "sqlx",
            Self::SqliteNative => "rusqlite",
            Self::Turso | Self::TursoMvcc => "turso",
        }
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::PostgresNative => "postgres-native",
            Self::PostgresSqlx => "postgres-sqlx",
            Self::SqliteNative => "sqlite-native",
            Self::SqliteSqlx => "sqlite-sqlx",
            Self::Turso => "turso",
            Self::TursoMvcc => "turso-mvcc",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Workload {
    BasicWrite,
    FanoutWrite,
    Retag,
    IndexedRead,
    JoinRead,
    Balanced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Durability {
    Durable,
    WriteOptimized,
}

impl fmt::Display for Durability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Durable => f.write_str("durable"),
            Self::WriteOptimized => f.write_str("write-optimized"),
        }
    }
}

impl Workload {
    pub const ALL: [Self; 6] = [
        Self::BasicWrite,
        Self::FanoutWrite,
        Self::Retag,
        Self::IndexedRead,
        Self::JoinRead,
        Self::Balanced,
    ];
}

impl fmt::Display for Workload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::BasicWrite => "basic-write",
            Self::FanoutWrite => "fanout-write",
            Self::Retag => "retag",
            Self::IndexedRead => "indexed-read",
            Self::JoinRead => "join-read",
            Self::Balanced => "balanced",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct FixtureConfig {
    pub users: u64,
    pub todos_per_user: u64,
    pub tags_per_user: u64,
    pub tags_per_todo: u64,
}

impl FixtureConfig {
    pub fn validate(&self) -> Result<()> {
        if self.users == 0 || self.todos_per_user == 0 {
            bail!("users and todos-per-user must be greater than zero");
        }
        if self.tags_per_user < 3 {
            bail!("tags-per-user must be at least 3");
        }
        if self.tags_per_todo < 2 || self.tags_per_todo > self.tags_per_user {
            bail!("tags-per-todo must be between 2 and tags-per-user");
        }
        if self.seed_todo_count() > i64::MAX as u64 {
            bail!("fixture is too large for signed 64-bit identifiers");
        }
        Ok(())
    }

    pub fn seed_todo_count(&self) -> u64 {
        self.users * self.todos_per_user
    }

    pub fn user_id(&self, sequence: u64) -> i64 {
        (sequence % self.users + 1) as i64
    }

    pub fn todo_id(&self, sequence: u64) -> i64 {
        let user_offset = sequence % self.users;
        let todo_offset = (sequence / self.users) % self.todos_per_user;
        (user_offset * self.todos_per_user + todo_offset + 1) as i64
    }

    pub fn tag_id(&self, user_id: i64, tag_offset: u64) -> i64 {
        ((user_id as u64 - 1) * self.tags_per_user + tag_offset % self.tags_per_user + 1) as i64
    }

    pub fn tag_ids_for_todo(&self, user_id: i64, todo_id: i64) -> Vec<i64> {
        let todo_offset = (todo_id as u64 - 1) % self.todos_per_user;
        (0..self.tags_per_todo)
            .map(|offset| self.tag_id(user_id, todo_offset + offset))
            .collect()
    }

    pub fn tag_pair(&self, sequence: u64) -> (i64, i64, i64) {
        let user_id = self.user_id(sequence);
        let todo_id = self.todo_id(sequence);
        let todo_offset = (todo_id as u64 - 1) % self.todos_per_user;
        (
            user_id,
            self.tag_id(user_id, todo_offset),
            self.tag_id(user_id, todo_offset + 1),
        )
    }
}

#[derive(Debug)]
pub enum Operation {
    Toggle {
        todo_id: i64,
        completed: bool,
        updated_at: i64,
    },
    InsertTagged {
        todo_id: i64,
        user_id: i64,
        title: String,
        timestamp: i64,
        tag_ids: Vec<i64>,
    },
    Retag {
        todo_id: i64,
        user_id: i64,
        tag_ids: Vec<i64>,
    },
    ListTodos {
        user_id: i64,
    },
    ListByTags {
        user_id: i64,
        tag_a: i64,
        tag_b: i64,
    },
}

pub fn parse_backends(value: &str) -> Result<Vec<BackendKind>> {
    if value.trim() == "all" {
        return Ok(BackendKind::ALL.to_vec());
    }
    value
        .split(',')
        .map(|item| {
            BackendKind::from_str(item.trim(), true)
                .map_err(|_| anyhow!("invalid backend {item:?}"))
        })
        .collect()
}

pub fn parse_workloads(value: &str) -> Result<Vec<Workload>> {
    if value.trim() == "all" {
        return Ok(Workload::ALL.to_vec());
    }
    value
        .split(',')
        .map(|item| {
            Workload::from_str(item.trim(), true).map_err(|_| anyhow!("invalid workload {item:?}"))
        })
        .collect()
}

pub fn parse_concurrency(value: &str) -> Result<Vec<usize>> {
    let values: Vec<usize> = value
        .split(',')
        .map(|item| {
            item.trim()
                .parse()
                .with_context(|| format!("invalid concurrency {item:?}"))
        })
        .collect::<Result<_>>()?;
    if values.is_empty() || values.contains(&0) {
        bail!("concurrency values must be greater than zero");
    }
    Ok(values)
}
