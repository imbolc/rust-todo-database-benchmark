use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection as RusqliteConnection, TransactionBehavior, params};
use sqlx::Connection as _;
use sqlx::postgres::{PgConnectOptions, PgConnection};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqliteSynchronous};
use tokio_postgres::{Client as PgClient, Config as PgConfig, NoTls, Statement as PgStatement};
use turso::{
    Connection as TursoConnection, Database as TursoDatabase, Statement as TursoStatement,
};

use crate::model::{BackendKind, Durability, FixtureConfig, Operation};
use crate::schema::{POSTGRES_SCHEMA, SQLITE_SCHEMA};

const BUSY_TIMEOUT: Duration = Duration::from_secs(30);
const SEED_USERS_PER_TRANSACTION: usize = 100;

pub struct Backend {
    kind: BackendKind,
    postgres_socket: PathBuf,
    postgres_port: u16,
    path: Option<PathBuf>,
    durability: Durability,
    turso: Option<Arc<TursoDatabase>>,
}

impl Backend {
    pub async fn open(
        kind: BackendKind,
        var_dir: &Path,
        postgres_port: u16,
        durability: Durability,
    ) -> Result<Self> {
        std::fs::create_dir_all(var_dir)
            .with_context(|| format!("create {}", var_dir.display()))?;

        let path = match kind {
            BackendKind::PostgresNative | BackendKind::PostgresSqlx => {
                Some(var_dir.join("postgres"))
            }
            BackendKind::SqliteNative => Some(var_dir.join("sqlite-native.db")),
            BackendKind::SqliteSqlx => Some(var_dir.join("sqlite-sqlx.db")),
            BackendKind::Turso => Some(var_dir.join("turso.db")),
            BackendKind::TursoMvcc => Some(var_dir.join("turso-mvcc.db")),
        };

        Ok(Self {
            kind,
            postgres_socket: var_dir.join("postgres-socket"),
            postgres_port,
            path,
            durability,
            turso: None,
        })
    }

    pub fn kind(&self) -> BackendKind {
        self.kind
    }

    pub fn storage(&self) -> String {
        self.path
            .as_ref()
            .map(|path| path.display().to_string())
            .expect("every backend has a storage path")
    }

    pub async fn reset_and_seed(&mut self, fixture: &FixtureConfig) -> Result<()> {
        match self.kind {
            BackendKind::PostgresNative => {
                let mut client =
                    connect_pg_native(&self.postgres_socket, self.postgres_port).await?;
                seed_pg_native(&mut client, fixture).await
            }
            BackendKind::PostgresSqlx => {
                let mut connection =
                    connect_pg_sqlx(&self.postgres_socket, self.postgres_port).await?;
                seed_pg_sqlx(&mut connection, fixture).await
            }
            BackendKind::SqliteNative => {
                let path = self.path.as_ref().expect("SQLite path is set");
                remove_embedded_database_files(path)?;
                let mut connection = open_rusqlite(path, self.durability)?;
                tokio::task::block_in_place(|| seed_rusqlite(&mut connection, fixture))
            }
            BackendKind::SqliteSqlx => {
                let path = self.path.as_ref().expect("SQLite path is set");
                remove_embedded_database_files(path)?;
                let mut connection = open_sqlx_sqlite(path, self.durability).await?;
                seed_sqlx_sqlite(&mut connection, fixture).await
            }
            BackendKind::Turso | BackendKind::TursoMvcc => {
                self.turso = None;
                let path = self.path.as_ref().expect("Turso path is set");
                remove_embedded_database_files(path)?;
                let path_string = path
                    .to_str()
                    .ok_or_else(|| anyhow!("Turso path is not valid UTF-8: {}", path.display()))?;
                let database = Arc::new(
                    turso::Builder::new_local(path_string)
                        .build()
                        .await
                        .context("open local Turso database")?,
                );
                let mut connection = database.connect().context("connect to Turso")?;
                configure_turso_synchronous(&connection, self.durability).await?;
                self.turso = Some(database);
                seed_turso(&mut connection, fixture).await?;
                set_turso_journal_mode(&connection, self.kind == BackendKind::TursoMvcc).await
            }
        }
    }

    pub async fn connect_worker(&self) -> Result<Worker> {
        match self.kind {
            BackendKind::PostgresNative => Ok(Worker::PostgresNative(
                PgNativeWorker::connect(&self.postgres_socket, self.postgres_port).await?,
            )),
            BackendKind::PostgresSqlx => Ok(Worker::PostgresSqlx(
                connect_pg_sqlx(&self.postgres_socket, self.postgres_port).await?,
            )),
            BackendKind::SqliteNative => {
                let path = self.path.as_ref().expect("SQLite path is set");
                Ok(Worker::SqliteNative(open_rusqlite(path, self.durability)?))
            }
            BackendKind::SqliteSqlx => {
                let path = self.path.as_ref().expect("SQLite path is set");
                Ok(Worker::SqliteSqlx(
                    open_sqlx_sqlite(path, self.durability).await?,
                ))
            }
            BackendKind::Turso | BackendKind::TursoMvcc => {
                let connection = self
                    .turso
                    .as_ref()
                    .expect("Turso database is set")
                    .connect()
                    .context("connect Turso benchmark worker")?;
                configure_turso_synchronous(&connection, self.durability).await?;
                verify_turso_journal_mode(&connection, self.kind == BackendKind::TursoMvcc).await?;
                Ok(Worker::Turso(
                    TursoWorker::from_connection(connection, self.kind == BackendKind::TursoMvcc)
                        .await?,
                ))
            }
        }
    }
}

pub enum Worker {
    PostgresNative(PgNativeWorker),
    PostgresSqlx(PgConnection),
    SqliteNative(RusqliteConnection),
    SqliteSqlx(SqliteConnection),
    Turso(TursoWorker),
}

impl Worker {
    pub async fn execute(&mut self, operation: &Operation) -> Result<()> {
        match self {
            Self::PostgresNative(worker) => worker.execute(operation).await,
            Self::PostgresSqlx(connection) => execute_pg_sqlx(connection, operation).await,
            Self::SqliteNative(connection) => {
                tokio::task::block_in_place(|| execute_rusqlite(connection, operation))
            }
            Self::SqliteSqlx(connection) => execute_sqlx_sqlite(connection, operation).await,
            Self::Turso(worker) => worker.execute(operation).await,
        }
    }
}

struct PgStatements {
    toggle: PgStatement,
    insert_todo: PgStatement,
    insert_tag: PgStatement,
    delete_tags: PgStatement,
    list_todos: PgStatement,
    list_by_tags: PgStatement,
}

pub struct PgNativeWorker {
    client: PgClient,
    statements: PgStatements,
}

impl PgNativeWorker {
    async fn connect(socket: &Path, port: u16) -> Result<Self> {
        let client = connect_pg_native(socket, port).await?;
        let statements = PgStatements {
            toggle: client
                .prepare("UPDATE todos SET completed = $2, updated_at = $3 WHERE id = $1")
                .await?,
            insert_todo: client
                .prepare(
                    "INSERT INTO todos \
                     (id, user_id, title, completed, created_at, updated_at) \
                     VALUES ($1, $2, $3, FALSE, $4, $4)",
                )
                .await?,
            insert_tag: client
                .prepare("INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES ($1, $2, $3)")
                .await?,
            delete_tags: client
                .prepare("DELETE FROM todo_tags WHERE todo_id = $1")
                .await?,
            list_todos: client
                .prepare(
                    "SELECT id, title, created_at FROM todos \
                     WHERE user_id = $1 AND completed = FALSE \
                     ORDER BY created_at DESC LIMIT 20",
                )
                .await?,
            list_by_tags: client
                .prepare(
                    "SELECT t.id, t.title, t.created_at FROM todos t \
                     JOIN todo_tags tt ON tt.todo_id = t.id AND tt.user_id = t.user_id \
                     WHERE t.user_id = $1 AND t.completed = FALSE \
                       AND tt.tag_id IN ($2, $3) \
                     GROUP BY t.id, t.title, t.created_at \
                     HAVING COUNT(DISTINCT tt.tag_id) = 2 \
                     ORDER BY t.created_at DESC LIMIT 20",
                )
                .await?,
        };
        Ok(Self { client, statements })
    }

    async fn execute(&mut self, operation: &Operation) -> Result<()> {
        match operation {
            Operation::Toggle {
                todo_id,
                completed,
                updated_at,
            } => {
                self.client
                    .execute(&self.statements.toggle, &[todo_id, completed, updated_at])
                    .await?;
            }
            Operation::InsertTagged {
                todo_id,
                user_id,
                title,
                timestamp,
                tag_ids,
            } => {
                let insert_todo = self.statements.insert_todo.clone();
                let insert_tag = self.statements.insert_tag.clone();
                let transaction = self.client.transaction().await?;
                transaction
                    .execute(&insert_todo, &[todo_id, user_id, title, timestamp])
                    .await?;
                for tag_id in tag_ids {
                    transaction
                        .execute(&insert_tag, &[user_id, todo_id, tag_id])
                        .await?;
                }
                transaction.commit().await?;
            }
            Operation::Retag {
                todo_id,
                user_id,
                tag_ids,
            } => {
                let delete_tags = self.statements.delete_tags.clone();
                let insert_tag = self.statements.insert_tag.clone();
                let transaction = self.client.transaction().await?;
                transaction.execute(&delete_tags, &[todo_id]).await?;
                for tag_id in tag_ids {
                    transaction
                        .execute(&insert_tag, &[user_id, todo_id, tag_id])
                        .await?;
                }
                transaction.commit().await?;
            }
            Operation::ListTodos { user_id } => {
                let rows = self
                    .client
                    .query(&self.statements.list_todos, &[user_id])
                    .await?;
                black_box(rows.len());
            }
            Operation::ListByTags {
                user_id,
                tag_a,
                tag_b,
            } => {
                let rows = self
                    .client
                    .query(&self.statements.list_by_tags, &[user_id, tag_a, tag_b])
                    .await?;
                black_box(rows.len());
            }
        }
        Ok(())
    }
}

struct TursoStatements {
    toggle: TursoStatement,
    insert_todo: TursoStatement,
    insert_tag: TursoStatement,
    delete_tags: TursoStatement,
    list_todos: TursoStatement,
    list_by_tags: TursoStatement,
}

pub struct TursoWorker {
    connection: TursoConnection,
    statements: TursoStatements,
    mvcc: bool,
}

impl TursoWorker {
    async fn from_connection(connection: TursoConnection, mvcc: bool) -> Result<Self> {
        let statements = TursoStatements {
            toggle: connection
                .prepare_cached("UPDATE todos SET completed = ?2, updated_at = ?3 WHERE id = ?1")
                .await?,
            insert_todo: connection
                .prepare_cached(
                    "INSERT INTO todos \
                     (id, user_id, title, completed, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, 0, ?4, ?4)",
                )
                .await?,
            insert_tag: connection
                .prepare_cached(
                    "INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES (?1, ?2, ?3)",
                )
                .await?,
            delete_tags: connection
                .prepare_cached("DELETE FROM todo_tags WHERE todo_id = ?1")
                .await?,
            list_todos: connection
                .prepare_cached(
                    "SELECT id, title, created_at FROM todos \
                     WHERE user_id = ?1 AND completed = 0 \
                     ORDER BY created_at DESC LIMIT 20",
                )
                .await?,
            list_by_tags: connection
                .prepare_cached(
                    "SELECT t.id, t.title, t.created_at FROM todos t \
                     JOIN todo_tags tt ON tt.todo_id = t.id AND tt.user_id = t.user_id \
                     WHERE t.user_id = ?1 AND t.completed = 0 \
                       AND tt.tag_id IN (?2, ?3) \
                     GROUP BY t.id, t.title, t.created_at \
                     HAVING COUNT(DISTINCT tt.tag_id) = 2 \
                     ORDER BY t.created_at DESC LIMIT 20",
                )
                .await?,
        };
        Ok(Self {
            connection,
            statements,
            mvcc,
        })
    }

    async fn execute(&mut self, operation: &Operation) -> Result<()> {
        match operation {
            Operation::Toggle {
                todo_id,
                completed,
                updated_at,
            } => {
                if self.mvcc {
                    return self
                        .execute_mvcc_toggle(*todo_id, *completed, *updated_at)
                        .await;
                }
                self.statements
                    .toggle
                    .execute(turso::params![*todo_id, i64::from(*completed), *updated_at])
                    .await?;
            }
            Operation::InsertTagged {
                todo_id,
                user_id,
                title,
                timestamp,
                tag_ids,
            } => {
                if self.mvcc {
                    return self
                        .execute_mvcc_insert_tagged(*todo_id, *user_id, title, *timestamp, tag_ids)
                        .await;
                }
                let mut insert_todo = self.statements.insert_todo.clone();
                let mut insert_tag = self.statements.insert_tag.clone();
                let transaction = self
                    .connection
                    .transaction_with_behavior(turso::transaction::TransactionBehavior::Immediate)
                    .await?;
                insert_todo
                    .execute(turso::params![
                        *todo_id,
                        *user_id,
                        title.as_str(),
                        *timestamp
                    ])
                    .await?;
                for tag_id in tag_ids {
                    insert_tag
                        .execute(turso::params![*user_id, *todo_id, *tag_id])
                        .await?;
                }
                transaction.commit().await?;
            }
            Operation::Retag {
                todo_id,
                user_id,
                tag_ids,
            } => {
                if self.mvcc {
                    return self.execute_mvcc_retag(*todo_id, *user_id, tag_ids).await;
                }
                let mut delete_tags = self.statements.delete_tags.clone();
                let mut insert_tag = self.statements.insert_tag.clone();
                let transaction = self
                    .connection
                    .transaction_with_behavior(turso::transaction::TransactionBehavior::Immediate)
                    .await?;
                delete_tags.execute(turso::params![*todo_id]).await?;
                for tag_id in tag_ids {
                    insert_tag
                        .execute(turso::params![*user_id, *todo_id, *tag_id])
                        .await?;
                }
                transaction.commit().await?;
            }
            Operation::ListTodos { user_id } => {
                let mut rows = self
                    .statements
                    .list_todos
                    .query(turso::params![*user_id])
                    .await?;
                let mut count = 0;
                while rows.next().await?.is_some() {
                    count += 1;
                }
                black_box(count);
            }
            Operation::ListByTags {
                user_id,
                tag_a,
                tag_b,
            } => {
                let mut rows = self
                    .statements
                    .list_by_tags
                    .query(turso::params![*user_id, *tag_a, *tag_b])
                    .await?;
                let mut count = 0;
                while rows.next().await?.is_some() {
                    count += 1;
                }
                black_box(count);
            }
        }
        Ok(())
    }

    async fn execute_mvcc_toggle(
        &mut self,
        todo_id: i64,
        completed: bool,
        updated_at: i64,
    ) -> Result<()> {
        loop {
            self.begin_concurrent().await?;
            let result = self
                .statements
                .toggle
                .execute(turso::params![todo_id, i64::from(completed), updated_at])
                .await
                .map(|_| ());
            if self.finish_concurrent(result).await? {
                return Ok(());
            }
        }
    }

    async fn execute_mvcc_insert_tagged(
        &mut self,
        todo_id: i64,
        user_id: i64,
        title: &str,
        timestamp: i64,
        tag_ids: &[i64],
    ) -> Result<()> {
        let mut insert_todo = self.statements.insert_todo.clone();
        let mut insert_tag = self.statements.insert_tag.clone();
        loop {
            self.begin_concurrent().await?;
            let result = async {
                insert_todo
                    .execute(turso::params![todo_id, user_id, title, timestamp])
                    .await?;
                for tag_id in tag_ids {
                    insert_tag
                        .execute(turso::params![user_id, todo_id, *tag_id])
                        .await?;
                }
                Ok::<_, turso::Error>(())
            }
            .await;
            if self.finish_concurrent(result).await? {
                return Ok(());
            }
        }
    }

    async fn execute_mvcc_retag(
        &mut self,
        todo_id: i64,
        user_id: i64,
        tag_ids: &[i64],
    ) -> Result<()> {
        let mut delete_tags = self.statements.delete_tags.clone();
        let mut insert_tag = self.statements.insert_tag.clone();
        loop {
            self.begin_concurrent().await?;
            let result = async {
                delete_tags.execute(turso::params![todo_id]).await?;
                for tag_id in tag_ids {
                    insert_tag
                        .execute(turso::params![user_id, todo_id, *tag_id])
                        .await?;
                }
                Ok::<_, turso::Error>(())
            }
            .await;
            if self.finish_concurrent(result).await? {
                return Ok(());
            }
        }
    }

    async fn begin_concurrent(&self) -> Result<()> {
        loop {
            match self.connection.execute("BEGIN CONCURRENT", ()).await {
                Ok(_) => return Ok(()),
                Err(error) if turso_error_is_retryable(&error) => tokio::task::yield_now().await,
                Err(error) => return Err(error.into()),
            }
        }
    }

    async fn finish_concurrent(&self, result: turso::Result<()>) -> Result<bool> {
        let result = match result {
            Ok(()) => self.connection.execute("COMMIT", ()).await.map(|_| ()),
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => Ok(true),
            Err(error) if turso_error_is_retryable(&error) => {
                let _ = self.connection.execute("ROLLBACK", ()).await;
                tokio::task::yield_now().await;
                Ok(false)
            }
            Err(error) => {
                let _ = self.connection.execute("ROLLBACK", ()).await;
                Err(error.into())
            }
        }
    }
}

fn turso_error_is_retryable(error: &turso::Error) -> bool {
    matches!(error, turso::Error::Busy(_) | turso::Error::BusySnapshot(_))
        || matches!(error, turso::Error::Error(message) if message.contains("conflict"))
}

async fn connect_pg_native(socket: &Path, port: u16) -> Result<PgClient> {
    let mut config = PgConfig::new();
    config
        .host_path(socket)
        .port(port)
        .user("postgres")
        .dbname("benchmark");
    let (client, connection) = config
        .connect(NoTls)
        .await
        .context("connect to PostgreSQL with tokio-postgres")?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("PostgreSQL connection task failed: {error}");
        }
    });
    Ok(client)
}

async fn connect_pg_sqlx(socket: &Path, port: u16) -> Result<PgConnection> {
    let options = PgConnectOptions::new()
        .socket(socket)
        .port(port)
        .username("postgres")
        .database("benchmark");
    PgConnection::connect_with(&options)
        .await
        .context("connect to PostgreSQL with SQLx")
}

fn sqlite_options(path: &Path, durability: Durability) -> SqliteConnectOptions {
    let synchronous = match durability {
        Durability::Durable => SqliteSynchronous::Full,
        Durability::WriteOptimized => SqliteSynchronous::Normal,
    };
    SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(synchronous)
        .busy_timeout(BUSY_TIMEOUT)
        .statement_cache_capacity(128)
}

async fn open_sqlx_sqlite(path: &Path, durability: Durability) -> Result<SqliteConnection> {
    SqliteConnection::connect_with(&sqlite_options(path, durability))
        .await
        .with_context(|| format!("open SQLite with SQLx at {}", path.display()))
}

fn open_rusqlite(path: &Path, durability: Durability) -> Result<RusqliteConnection> {
    let connection = RusqliteConnection::open(path)
        .with_context(|| format!("open SQLite with rusqlite at {}", path.display()))?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let synchronous = match durability {
        Durability::Durable => "FULL",
        Durability::WriteOptimized => "NORMAL",
    };
    connection.pragma_update(None, "synchronous", synchronous)?;
    connection.set_prepared_statement_cache_capacity(128);
    Ok(connection)
}

async fn configure_turso_synchronous(
    connection: &TursoConnection,
    durability: Durability,
) -> Result<()> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let (synchronous, expected) = match durability {
        Durability::Durable => ("FULL", 2),
        Durability::WriteOptimized => ("NORMAL", 1),
    };
    connection
        .execute_batch(format!("PRAGMA synchronous = {synchronous}"))
        .await?;
    let mut rows = connection.query("PRAGMA synchronous", ()).await?;
    let actual = rows
        .next()
        .await?
        .ok_or_else(|| anyhow!("Turso PRAGMA synchronous returned no value"))?
        .get::<i64>(0)?;
    if actual != expected {
        return Err(anyhow!(
            "Turso synchronous mode is {actual}, expected {expected} ({synchronous})"
        ));
    }
    Ok(())
}

async fn set_turso_journal_mode(connection: &TursoConnection, mvcc: bool) -> Result<()> {
    let journal_mode = if mvcc { "mvcc" } else { "wal" };
    connection
        .pragma_update("journal_mode", format!("'{journal_mode}'"))
        .await?;
    verify_turso_journal_mode(connection, mvcc).await
}

async fn verify_turso_journal_mode(connection: &TursoConnection, mvcc: bool) -> Result<()> {
    let expected = if mvcc { "mvcc" } else { "wal" };
    let mut rows = connection.query("PRAGMA journal_mode", ()).await?;
    let actual = rows
        .next()
        .await?
        .ok_or_else(|| anyhow!("Turso PRAGMA journal_mode returned no value"))?
        .get::<String>(0)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(anyhow!(
            "Turso journal mode is {actual}, expected {expected}"
        ));
    }
    Ok(())
}

fn remove_embedded_database_files(path: &Path) -> Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal", ".wal", "-log"] {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(suffix);
        let candidate = PathBuf::from(candidate);
        match std::fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("remove {}", candidate.display()));
            }
        }
    }
    Ok(())
}

async fn seed_pg_native(client: &mut PgClient, fixture: &FixtureConfig) -> Result<()> {
    client.batch_execute(POSTGRES_SCHEMA).await?;
    for first_user in (1..=fixture.users).step_by(SEED_USERS_PER_TRANSACTION) {
        let last_user = (first_user + SEED_USERS_PER_TRANSACTION as u64 - 1).min(fixture.users);
        let transaction = client.transaction().await?;
        let insert_user = transaction
            .prepare("INSERT INTO users (id, email, created_at) VALUES ($1, $2, $3)")
            .await?;
        let insert_todo = transaction
            .prepare(
                "INSERT INTO todos \
                 (id, user_id, title, completed, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $5)",
            )
            .await?;
        let insert_tag = transaction
            .prepare("INSERT INTO tags (id, user_id, name) VALUES ($1, $2, $3)")
            .await?;
        let link_tag = transaction
            .prepare("INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES ($1, $2, $3)")
            .await?;

        for user_id in first_user as i64..=last_user as i64 {
            let email = format!("user-{user_id}@example.test");
            transaction
                .execute(
                    &insert_user,
                    &[&user_id, &email, &(1_700_000_000 + user_id)],
                )
                .await?;
            for tag_offset in 0..fixture.tags_per_user {
                let tag_id = fixture.tag_id(user_id, tag_offset);
                let name = format!("tag-{tag_offset}");
                transaction
                    .execute(&insert_tag, &[&tag_id, &user_id, &name])
                    .await?;
            }
            for todo_offset in 0..fixture.todos_per_user {
                let todo_id =
                    (user_id - 1) * fixture.todos_per_user as i64 + todo_offset as i64 + 1;
                let title = format!("Todo {todo_id} for user {user_id}");
                let completed = todo_offset.is_multiple_of(3);
                let timestamp = 1_700_000_000 + todo_id;
                transaction
                    .execute(
                        &insert_todo,
                        &[&todo_id, &user_id, &title, &completed, &timestamp],
                    )
                    .await?;
                for tag_id in fixture.tag_ids_for_todo(user_id, todo_id) {
                    transaction
                        .execute(&link_tag, &[&user_id, &todo_id, &tag_id])
                        .await?;
                }
            }
        }
        transaction.commit().await?;
    }
    Ok(())
}

async fn seed_pg_sqlx(connection: &mut PgConnection, fixture: &FixtureConfig) -> Result<()> {
    sqlx::raw_sql(POSTGRES_SCHEMA)
        .execute(&mut *connection)
        .await?;
    for first_user in (1..=fixture.users).step_by(SEED_USERS_PER_TRANSACTION) {
        let last_user = (first_user + SEED_USERS_PER_TRANSACTION as u64 - 1).min(fixture.users);
        let mut transaction = connection.begin().await?;
        for user_id in first_user as i64..=last_user as i64 {
            sqlx::query("INSERT INTO users (id, email, created_at) VALUES ($1, $2, $3)")
                .bind(user_id)
                .bind(format!("user-{user_id}@example.test"))
                .bind(1_700_000_000 + user_id)
                .execute(&mut *transaction)
                .await?;
            for tag_offset in 0..fixture.tags_per_user {
                sqlx::query("INSERT INTO tags (id, user_id, name) VALUES ($1, $2, $3)")
                    .bind(fixture.tag_id(user_id, tag_offset))
                    .bind(user_id)
                    .bind(format!("tag-{tag_offset}"))
                    .execute(&mut *transaction)
                    .await?;
            }
            for todo_offset in 0..fixture.todos_per_user {
                let todo_id =
                    (user_id - 1) * fixture.todos_per_user as i64 + todo_offset as i64 + 1;
                sqlx::query(
                    "INSERT INTO todos \
                     (id, user_id, title, completed, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $5)",
                )
                .bind(todo_id)
                .bind(user_id)
                .bind(format!("Todo {todo_id} for user {user_id}"))
                .bind(todo_offset.is_multiple_of(3))
                .bind(1_700_000_000 + todo_id)
                .execute(&mut *transaction)
                .await?;
                for tag_id in fixture.tag_ids_for_todo(user_id, todo_id) {
                    sqlx::query(
                        "INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES ($1, $2, $3)",
                    )
                    .bind(user_id)
                    .bind(todo_id)
                    .bind(tag_id)
                    .execute(&mut *transaction)
                    .await?;
                }
            }
        }
        transaction.commit().await?;
    }
    Ok(())
}

fn seed_rusqlite(connection: &mut RusqliteConnection, fixture: &FixtureConfig) -> Result<()> {
    connection.execute_batch(SQLITE_SCHEMA)?;
    for first_user in (1..=fixture.users).step_by(SEED_USERS_PER_TRANSACTION) {
        let last_user = (first_user + SEED_USERS_PER_TRANSACTION as u64 - 1).min(fixture.users);
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut insert_user = transaction
                .prepare_cached("INSERT INTO users (id, email, created_at) VALUES (?1, ?2, ?3)")?;
            let mut insert_todo = transaction.prepare_cached(
                "INSERT INTO todos \
                 (id, user_id, title, completed, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            )?;
            let mut insert_tag = transaction
                .prepare_cached("INSERT INTO tags (id, user_id, name) VALUES (?1, ?2, ?3)")?;
            let mut link_tag = transaction.prepare_cached(
                "INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES (?1, ?2, ?3)",
            )?;
            for user_id in first_user as i64..=last_user as i64 {
                insert_user.execute(params![
                    user_id,
                    format!("user-{user_id}@example.test"),
                    1_700_000_000 + user_id
                ])?;
                for tag_offset in 0..fixture.tags_per_user {
                    insert_tag.execute(params![
                        fixture.tag_id(user_id, tag_offset),
                        user_id,
                        format!("tag-{tag_offset}")
                    ])?;
                }
                for todo_offset in 0..fixture.todos_per_user {
                    let todo_id =
                        (user_id - 1) * fixture.todos_per_user as i64 + todo_offset as i64 + 1;
                    insert_todo.execute(params![
                        todo_id,
                        user_id,
                        format!("Todo {todo_id} for user {user_id}"),
                        i64::from(todo_offset.is_multiple_of(3)),
                        1_700_000_000 + todo_id
                    ])?;
                    for tag_id in fixture.tag_ids_for_todo(user_id, todo_id) {
                        link_tag.execute(params![user_id, todo_id, tag_id])?;
                    }
                }
            }
        }
        transaction.commit()?;
    }
    Ok(())
}

async fn seed_sqlx_sqlite(
    connection: &mut SqliteConnection,
    fixture: &FixtureConfig,
) -> Result<()> {
    sqlx::raw_sql(SQLITE_SCHEMA)
        .execute(&mut *connection)
        .await?;
    for first_user in (1..=fixture.users).step_by(SEED_USERS_PER_TRANSACTION) {
        let last_user = (first_user + SEED_USERS_PER_TRANSACTION as u64 - 1).min(fixture.users);
        let mut transaction = connection.begin().await?;
        for user_id in first_user as i64..=last_user as i64 {
            sqlx::query("INSERT INTO users (id, email, created_at) VALUES (?1, ?2, ?3)")
                .bind(user_id)
                .bind(format!("user-{user_id}@example.test"))
                .bind(1_700_000_000 + user_id)
                .execute(&mut *transaction)
                .await?;
            for tag_offset in 0..fixture.tags_per_user {
                sqlx::query("INSERT INTO tags (id, user_id, name) VALUES (?1, ?2, ?3)")
                    .bind(fixture.tag_id(user_id, tag_offset))
                    .bind(user_id)
                    .bind(format!("tag-{tag_offset}"))
                    .execute(&mut *transaction)
                    .await?;
            }
            for todo_offset in 0..fixture.todos_per_user {
                let todo_id =
                    (user_id - 1) * fixture.todos_per_user as i64 + todo_offset as i64 + 1;
                sqlx::query(
                    "INSERT INTO todos \
                     (id, user_id, title, completed, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                )
                .bind(todo_id)
                .bind(user_id)
                .bind(format!("Todo {todo_id} for user {user_id}"))
                .bind(i64::from(todo_offset.is_multiple_of(3)))
                .bind(1_700_000_000 + todo_id)
                .execute(&mut *transaction)
                .await?;
                for tag_id in fixture.tag_ids_for_todo(user_id, todo_id) {
                    sqlx::query(
                        "INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES (?1, ?2, ?3)",
                    )
                    .bind(user_id)
                    .bind(todo_id)
                    .bind(tag_id)
                    .execute(&mut *transaction)
                    .await?;
                }
            }
        }
        transaction.commit().await?;
    }
    Ok(())
}

async fn seed_turso(connection: &mut TursoConnection, fixture: &FixtureConfig) -> Result<()> {
    connection.execute_batch(SQLITE_SCHEMA).await?;
    for first_user in (1..=fixture.users).step_by(SEED_USERS_PER_TRANSACTION) {
        let last_user = (first_user + SEED_USERS_PER_TRANSACTION as u64 - 1).min(fixture.users);
        let transaction = connection
            .transaction_with_behavior(turso::transaction::TransactionBehavior::Immediate)
            .await?;
        let mut insert_user = transaction
            .prepare("INSERT INTO users (id, email, created_at) VALUES (?1, ?2, ?3)")
            .await?;
        let mut insert_todo = transaction
            .prepare(
                "INSERT INTO todos \
                 (id, user_id, title, completed, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            )
            .await?;
        let mut insert_tag = transaction
            .prepare("INSERT INTO tags (id, user_id, name) VALUES (?1, ?2, ?3)")
            .await?;
        let mut link_tag = transaction
            .prepare("INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES (?1, ?2, ?3)")
            .await?;
        for user_id in first_user as i64..=last_user as i64 {
            insert_user
                .execute(turso::params![
                    user_id,
                    format!("user-{user_id}@example.test"),
                    1_700_000_000 + user_id
                ])
                .await?;
            for tag_offset in 0..fixture.tags_per_user {
                insert_tag
                    .execute(turso::params![
                        fixture.tag_id(user_id, tag_offset),
                        user_id,
                        format!("tag-{tag_offset}")
                    ])
                    .await?;
            }
            for todo_offset in 0..fixture.todos_per_user {
                let todo_id =
                    (user_id - 1) * fixture.todos_per_user as i64 + todo_offset as i64 + 1;
                insert_todo
                    .execute(turso::params![
                        todo_id,
                        user_id,
                        format!("Todo {todo_id} for user {user_id}"),
                        i64::from(todo_offset.is_multiple_of(3)),
                        1_700_000_000 + todo_id
                    ])
                    .await?;
                for tag_id in fixture.tag_ids_for_todo(user_id, todo_id) {
                    link_tag
                        .execute(turso::params![user_id, todo_id, tag_id])
                        .await?;
                }
            }
        }
        drop(insert_user);
        drop(insert_todo);
        drop(insert_tag);
        drop(link_tag);
        transaction.commit().await?;
    }
    Ok(())
}

async fn execute_pg_sqlx(connection: &mut PgConnection, operation: &Operation) -> Result<()> {
    match operation {
        Operation::Toggle {
            todo_id,
            completed,
            updated_at,
        } => {
            sqlx::query("UPDATE todos SET completed = $2, updated_at = $3 WHERE id = $1")
                .bind(todo_id)
                .bind(completed)
                .bind(updated_at)
                .execute(&mut *connection)
                .await?;
        }
        Operation::InsertTagged {
            todo_id,
            user_id,
            title,
            timestamp,
            tag_ids,
        } => {
            let mut transaction = connection.begin().await?;
            sqlx::query(
                "INSERT INTO todos \
                 (id, user_id, title, completed, created_at, updated_at) \
                 VALUES ($1, $2, $3, FALSE, $4, $4)",
            )
            .bind(todo_id)
            .bind(user_id)
            .bind(title)
            .bind(timestamp)
            .execute(&mut *transaction)
            .await?;
            for tag_id in tag_ids {
                sqlx::query("INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES ($1, $2, $3)")
                    .bind(user_id)
                    .bind(todo_id)
                    .bind(tag_id)
                    .execute(&mut *transaction)
                    .await?;
            }
            transaction.commit().await?;
        }
        Operation::Retag {
            todo_id,
            user_id,
            tag_ids,
        } => {
            let mut transaction = connection.begin().await?;
            sqlx::query("DELETE FROM todo_tags WHERE todo_id = $1")
                .bind(todo_id)
                .execute(&mut *transaction)
                .await?;
            for tag_id in tag_ids {
                sqlx::query("INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES ($1, $2, $3)")
                    .bind(user_id)
                    .bind(todo_id)
                    .bind(tag_id)
                    .execute(&mut *transaction)
                    .await?;
            }
            transaction.commit().await?;
        }
        Operation::ListTodos { user_id } => {
            let rows = sqlx::query(
                "SELECT id, title, created_at FROM todos \
                 WHERE user_id = $1 AND completed = FALSE \
                 ORDER BY created_at DESC LIMIT 20",
            )
            .bind(user_id)
            .fetch_all(&mut *connection)
            .await?;
            black_box(rows.len());
        }
        Operation::ListByTags {
            user_id,
            tag_a,
            tag_b,
        } => {
            let rows = sqlx::query(
                "SELECT t.id, t.title, t.created_at FROM todos t \
                 JOIN todo_tags tt ON tt.todo_id = t.id AND tt.user_id = t.user_id \
                 WHERE t.user_id = $1 AND t.completed = FALSE \
                   AND tt.tag_id IN ($2, $3) \
                 GROUP BY t.id, t.title, t.created_at \
                 HAVING COUNT(DISTINCT tt.tag_id) = 2 \
                 ORDER BY t.created_at DESC LIMIT 20",
            )
            .bind(user_id)
            .bind(tag_a)
            .bind(tag_b)
            .fetch_all(&mut *connection)
            .await?;
            black_box(rows.len());
        }
    }
    Ok(())
}

fn execute_rusqlite(connection: &mut RusqliteConnection, operation: &Operation) -> Result<()> {
    match operation {
        Operation::Toggle {
            todo_id,
            completed,
            updated_at,
        } => {
            connection
                .prepare_cached("UPDATE todos SET completed = ?2, updated_at = ?3 WHERE id = ?1")?
                .execute(params![todo_id, i64::from(*completed), updated_at])?;
        }
        Operation::InsertTagged {
            todo_id,
            user_id,
            title,
            timestamp,
            tag_ids,
        } => {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction
                .prepare_cached(
                    "INSERT INTO todos \
                     (id, user_id, title, completed, created_at, updated_at) \
                     VALUES (?1, ?2, ?3, 0, ?4, ?4)",
                )?
                .execute(params![todo_id, user_id, title, timestamp])?;
            for tag_id in tag_ids {
                transaction
                    .prepare_cached(
                        "INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES (?1, ?2, ?3)",
                    )?
                    .execute(params![user_id, todo_id, tag_id])?;
            }
            transaction.commit()?;
        }
        Operation::Retag {
            todo_id,
            user_id,
            tag_ids,
        } => {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction
                .prepare_cached("DELETE FROM todo_tags WHERE todo_id = ?1")?
                .execute(params![todo_id])?;
            for tag_id in tag_ids {
                transaction
                    .prepare_cached(
                        "INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES (?1, ?2, ?3)",
                    )?
                    .execute(params![user_id, todo_id, tag_id])?;
            }
            transaction.commit()?;
        }
        Operation::ListTodos { user_id } => {
            let mut statement = connection.prepare_cached(
                "SELECT id, title, created_at FROM todos \
                 WHERE user_id = ?1 AND completed = 0 \
                 ORDER BY created_at DESC LIMIT 20",
            )?;
            let mut rows = statement.query(params![user_id])?;
            let mut count = 0;
            while rows.next()?.is_some() {
                count += 1;
            }
            black_box(count);
        }
        Operation::ListByTags {
            user_id,
            tag_a,
            tag_b,
        } => {
            let mut statement = connection.prepare_cached(
                "SELECT t.id, t.title, t.created_at FROM todos t \
                 JOIN todo_tags tt ON tt.todo_id = t.id AND tt.user_id = t.user_id \
                 WHERE t.user_id = ?1 AND t.completed = 0 \
                   AND tt.tag_id IN (?2, ?3) \
                 GROUP BY t.id, t.title, t.created_at \
                 HAVING COUNT(DISTINCT tt.tag_id) = 2 \
                 ORDER BY t.created_at DESC LIMIT 20",
            )?;
            let mut rows = statement.query(params![user_id, tag_a, tag_b])?;
            let mut count = 0;
            while rows.next()?.is_some() {
                count += 1;
            }
            black_box(count);
        }
    }
    Ok(())
}

async fn execute_sqlx_sqlite(
    connection: &mut SqliteConnection,
    operation: &Operation,
) -> Result<()> {
    match operation {
        Operation::Toggle {
            todo_id,
            completed,
            updated_at,
        } => {
            sqlx::query("UPDATE todos SET completed = ?2, updated_at = ?3 WHERE id = ?1")
                .bind(todo_id)
                .bind(i64::from(*completed))
                .bind(updated_at)
                .execute(&mut *connection)
                .await?;
        }
        Operation::InsertTagged {
            todo_id,
            user_id,
            title,
            timestamp,
            tag_ids,
        } => {
            let mut transaction = connection.begin().await?;
            sqlx::query(
                "INSERT INTO todos \
                 (id, user_id, title, completed, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, 0, ?4, ?4)",
            )
            .bind(todo_id)
            .bind(user_id)
            .bind(title)
            .bind(timestamp)
            .execute(&mut *transaction)
            .await?;
            for tag_id in tag_ids {
                sqlx::query("INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES (?1, ?2, ?3)")
                    .bind(user_id)
                    .bind(todo_id)
                    .bind(tag_id)
                    .execute(&mut *transaction)
                    .await?;
            }
            transaction.commit().await?;
        }
        Operation::Retag {
            todo_id,
            user_id,
            tag_ids,
        } => {
            let mut transaction = connection.begin().await?;
            sqlx::query("DELETE FROM todo_tags WHERE todo_id = ?1")
                .bind(todo_id)
                .execute(&mut *transaction)
                .await?;
            for tag_id in tag_ids {
                sqlx::query("INSERT INTO todo_tags (user_id, todo_id, tag_id) VALUES (?1, ?2, ?3)")
                    .bind(user_id)
                    .bind(todo_id)
                    .bind(tag_id)
                    .execute(&mut *transaction)
                    .await?;
            }
            transaction.commit().await?;
        }
        Operation::ListTodos { user_id } => {
            let rows = sqlx::query(
                "SELECT id, title, created_at FROM todos \
                 WHERE user_id = ?1 AND completed = 0 \
                 ORDER BY created_at DESC LIMIT 20",
            )
            .bind(user_id)
            .fetch_all(&mut *connection)
            .await?;
            black_box(rows.len());
        }
        Operation::ListByTags {
            user_id,
            tag_a,
            tag_b,
        } => {
            let rows = sqlx::query(
                "SELECT t.id, t.title, t.created_at FROM todos t \
                 JOIN todo_tags tt ON tt.todo_id = t.id AND tt.user_id = t.user_id \
                 WHERE t.user_id = ?1 AND t.completed = 0 \
                   AND tt.tag_id IN (?2, ?3) \
                 GROUP BY t.id, t.title, t.created_at \
                 HAVING COUNT(DISTINCT tt.tag_id) = 2 \
                 ORDER BY t.created_at DESC LIMIT 20",
            )
            .bind(user_id)
            .bind(tag_a)
            .bind(tag_b)
            .fetch_all(&mut *connection)
            .await?;
            black_box(rows.len());
        }
    }
    Ok(())
}
