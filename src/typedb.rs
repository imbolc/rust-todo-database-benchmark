use std::hint::black_box;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use futures_util::TryStreamExt;
use typedb_driver::answer::QueryAnswer;
use typedb_driver::given::{GivenRowEntry, GivenRows};
use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, Transaction, TransactionType,
    TypeDBDriver,
};

use crate::model::{FixtureConfig, Operation};

#[derive(Clone, clap::Args)]
pub struct TypeDbConfig {
    #[arg(
        long = "typedb-address",
        env = "TYPEDB_ADDRESS",
        default_value = "localhost:1729"
    )]
    address: String,

    /// Dedicated benchmark database, deleted and recreated when seeding.
    #[arg(
        long = "typedb-database",
        env = "TYPEDB_DATABASE",
        default_value = "rust-todo-benchmark"
    )]
    database: String,

    #[arg(
        long = "typedb-username",
        env = "TYPEDB_USERNAME",
        default_value = "admin"
    )]
    username: String,

    #[arg(
        long = "typedb-password",
        env = "TYPEDB_PASSWORD",
        default_value = "password",
        hide_default_value = true,
        hide_env_values = true
    )]
    password: String,

    /// Enable TLS using the system's trusted certificates.
    #[arg(long = "typedb-tls")]
    tls: bool,
}

// Required role players preserve ownership even when no tags are attached.
const SCHEMA: &str = r#"
define
attribute user-id, value integer;
attribute todo-id, value integer;
attribute tag-id, value integer;
attribute email, value string;
attribute title, value string;
attribute completed, value boolean;
attribute created-at, value integer;
attribute updated-at, value integer;
attribute name, value string;
attribute tag-key, value string;
attribute todo-tag-key, value string;

entity user,
    owns user-id @key,
    owns email @key,
    owns created-at @card(1),
    plays todo:owner,
    plays tag:owner;

relation todo,
    relates owner @card(1),
    owns todo-id @key,
    owns title @card(1),
    owns completed @card(1),
    owns created-at @card(1),
    owns updated-at @card(1),
    plays todo-tag:todo;

relation tag,
    relates owner @card(1),
    owns tag-id @key,
    owns name @card(1),
    owns tag-key @key,
    plays todo-tag:tag;

relation todo-tag,
    relates todo @card(1),
    relates tag @card(1),
    owns todo-tag-key @key;
"#;

const INSERT_USER: &str = "\
given $id: integer, $email: string, $created: integer; \
insert $u isa user, has user-id == $id, has email == $email, has created-at == $created; \
select $u;";

const INSERT_TAG: &str = "\
given $id: integer, $uid: integer, $name: string, $key: string; \
match $u isa user, has user-id == $uid; \
insert $g isa tag, links (owner: $u), has tag-id == $id, has name == $name, \
    has tag-key == $key; \
select $g;";

const INSERT_TODO: &str = "\
given $id: integer, $uid: integer, $title: string, $done: boolean, $created: integer; \
match $u isa user, has user-id == $uid; \
insert $t isa todo, links (owner: $u), has todo-id == $id, has title == $title, \
    has completed == $done, has created-at == $created, has updated-at == $created; \
select $t;";

const LINK_TAG: &str = "\
given $tid: integer, $uid: integer, $gid: integer, $key: string; \
match \
    $u isa user, has user-id == $uid; \
    $t isa todo, links (owner: $u), has todo-id == $tid; \
    $g isa tag, links (owner: $u), has tag-id == $gid; \
insert $link isa todo-tag, links (todo: $t, tag: $g), has todo-tag-key == $key; \
select $link;";

const TOGGLE: &str = "\
given $id: integer, $done: boolean, $updated: integer; \
match $t isa todo, has todo-id == $id; \
update $t has completed == $done, has updated-at == $updated; \
select $t;";

const DELETE_TAGS: &str = "\
given $tid: integer; \
match $t isa todo, has todo-id == $tid; $link isa todo-tag, links (todo: $t); \
delete $link;";

const LIST_TODOS: &str = "\
given $uid: integer; \
match \
    $u isa user, has user-id == $uid; \
    $t isa todo, links (owner: $u), has completed false, \
        has todo-id $id, has title $title, has created-at $created; \
select $id, $title, $created; sort $created desc; limit 20;";

const LIST_BY_TAGS: &str = "\
given $uid: integer, $a: integer, $b: integer; \
match \
    $a != $b; \
    $u isa user, has user-id == $uid; \
    $t isa todo, links (owner: $u), has completed false, \
        has todo-id $id, has title $title, has created-at $created; \
    $ga isa tag, links (owner: $u), has tag-id == $a; \
    $gb isa tag, links (owner: $u), has tag-id == $b; \
    (todo: $t, tag: $ga) isa todo-tag; \
    (todo: $t, tag: $gb) isa todo-tag; \
select $id, $title, $created; distinct; sort $created desc; limit 20;";

#[derive(Clone)]
pub struct TypeDb {
    driver: Arc<TypeDBDriver>,
    database: String,
    address: String,
}

impl TypeDb {
    pub async fn connect(config: &TypeDbConfig) -> Result<Self> {
        let tls = if config.tls {
            DriverTlsConfig::enabled_with_native_root_ca()
        } else {
            DriverTlsConfig::disabled()
        };
        let driver = TypeDBDriver::new(
            Addresses::try_from_address_str(&config.address)?,
            Credentials::new(&config.username, &config.password),
            DriverOptions::new(tls),
        )
        .await
        .with_context(|| {
            format!(
                "connect to TypeDB at {}; start the local TypeDB server first",
                config.address
            )
        })?;
        Ok(Self {
            driver: Arc::new(driver),
            database: config.database.clone(),
            address: config.address.clone(),
        })
    }

    pub fn storage(&self) -> String {
        format!("{}/{}", self.address, self.database)
    }

    pub async fn reset_and_seed(&self, fixture: &FixtureConfig) -> Result<()> {
        if self.driver.databases().contains(&self.database).await? {
            self.driver
                .databases()
                .get(&self.database)
                .await?
                .delete()
                .await?;
        }
        self.driver.databases().create(&self.database).await?;
        let schema = self
            .driver
            .transaction(&self.database, TransactionType::Schema)
            .await?;
        schema.query(SCHEMA).await?;
        schema.commit().await?;

        for user_id in 1..=fixture.users as i64 {
            let transaction = self
                .driver
                .transaction(&self.database, TransactionType::Write)
                .await?;
            query(
                &transaction,
                INSERT_USER,
                given(
                    &["id", "email", "created"],
                    vec![
                        user_id.into(),
                        format!("user-{user_id}@example.test").into(),
                        (1_700_000_000 + user_id).into(),
                    ],
                )?,
            )
            .await?;

            let mut tags = GivenRows::new(
                strings(&["id", "uid", "name", "key"]),
                fixture.tags_per_user as usize,
            );
            for offset in 0..fixture.tags_per_user {
                tags.push_row(vec![
                    fixture.tag_id(user_id, offset).into(),
                    user_id.into(),
                    format!("tag-{offset}").into(),
                    format!("{user_id}:tag-{offset}").into(),
                ])?;
            }
            query(&transaction, INSERT_TAG, tags).await?;

            let mut todos = GivenRows::new(
                strings(&["id", "uid", "title", "done", "created"]),
                fixture.todos_per_user as usize,
            );
            let mut links = GivenRows::new(strings(&["tid", "uid", "gid", "key"]), 0);
            for offset in 0..fixture.todos_per_user {
                let todo_id = (user_id - 1) * fixture.todos_per_user as i64 + offset as i64 + 1;
                todos.push_row(vec![
                    todo_id.into(),
                    user_id.into(),
                    format!("Todo {todo_id} for user {user_id}").into(),
                    offset.is_multiple_of(3).into(),
                    (1_700_000_000 + todo_id).into(),
                ])?;
                for tag_id in fixture.tag_ids_for_todo(user_id, todo_id) {
                    links.push_row(tag_link(todo_id, user_id, tag_id))?;
                }
            }
            query(&transaction, INSERT_TODO, todos).await?;
            query(&transaction, LINK_TAG, links).await?;
            transaction.commit().await?;
        }
        Ok(())
    }

    pub async fn execute(&self, operation: &Operation) -> Result<()> {
        let read = matches!(
            operation,
            Operation::ListTodos { .. } | Operation::ListByTags { .. }
        );
        let transaction = self
            .driver
            .transaction(
                &self.database,
                if read {
                    TransactionType::Read
                } else {
                    TransactionType::Write
                },
            )
            .await?;
        match operation {
            Operation::Toggle {
                todo_id,
                completed,
                updated_at,
            } => {
                query(
                    &transaction,
                    TOGGLE,
                    given(
                        &["id", "done", "updated"],
                        vec![(*todo_id).into(), (*completed).into(), (*updated_at).into()],
                    )?,
                )
                .await?;
            }
            Operation::InsertTagged {
                todo_id,
                user_id,
                title,
                timestamp,
                tag_ids,
            } => {
                let inserted = query(
                    &transaction,
                    INSERT_TODO,
                    given(
                        &["id", "uid", "title", "done", "created"],
                        vec![
                            (*todo_id).into(),
                            (*user_id).into(),
                            title.as_str().into(),
                            false.into(),
                            (*timestamp).into(),
                        ],
                    )?,
                )
                .await?;
                ensure!(inserted == 1, "TypeDB todo owner {user_id} does not exist");
                link_tags(&transaction, *todo_id, *user_id, tag_ids).await?;
            }
            Operation::Retag {
                todo_id,
                user_id,
                tag_ids,
            } => {
                query(
                    &transaction,
                    DELETE_TAGS,
                    given(&["tid"], vec![(*todo_id).into()])?,
                )
                .await?;
                link_tags(&transaction, *todo_id, *user_id, tag_ids).await?;
            }
            Operation::ListTodos { user_id } => {
                query(
                    &transaction,
                    LIST_TODOS,
                    given(&["uid"], vec![(*user_id).into()])?,
                )
                .await?;
            }
            Operation::ListByTags {
                user_id,
                tag_a,
                tag_b,
            } => {
                query(
                    &transaction,
                    LIST_BY_TAGS,
                    given(
                        &["uid", "a", "b"],
                        vec![(*user_id).into(), (*tag_a).into(), (*tag_b).into()],
                    )?,
                )
                .await?;
            }
        }
        if read {
            transaction.close().await?;
        } else {
            transaction.commit().await?;
        }
        Ok(())
    }
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn given(names: &[&str], values: Vec<GivenRowEntry>) -> Result<GivenRows> {
    let mut rows = GivenRows::new(strings(names), 1);
    rows.push_row(values)?;
    Ok(rows)
}

fn tag_link(todo_id: i64, user_id: i64, tag_id: i64) -> Vec<GivenRowEntry> {
    vec![
        todo_id.into(),
        user_id.into(),
        tag_id.into(),
        format!("{todo_id}:{tag_id}").into(),
    ]
}

async fn link_tags(
    transaction: &Transaction,
    todo_id: i64,
    user_id: i64,
    tag_ids: &[i64],
) -> Result<()> {
    if tag_ids.is_empty() {
        return Ok(());
    }
    let mut links = GivenRows::new(strings(&["tid", "uid", "gid", "key"]), tag_ids.len());
    for &tag_id in tag_ids {
        links.push_row(tag_link(todo_id, user_id, tag_id))?;
    }
    let linked = query(transaction, LINK_TAG, links).await?;
    ensure!(
        linked == tag_ids.len(),
        "TypeDB todo {todo_id} or a tag does not belong to user {user_id}"
    );
    Ok(())
}

async fn query(transaction: &Transaction, query: &str, rows: GivenRows) -> Result<usize> {
    let answer = transaction.query_with_rows(query, rows).await?;
    let QueryAnswer::ConceptRowStream(_, mut rows) = answer else {
        anyhow::bail!("TypeDB data query did not return rows");
    };
    let mut count = 0;
    // Errors can arrive after the initial answer, so finish the stream before committing.
    while let Some(row) = rows.try_next().await? {
        black_box(row);
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use typedb_driver::answer::ConceptRow;

    use super::*;

    async fn read(db: &TypeDb, query: &str, rows: GivenRows) -> Result<Vec<ConceptRow>> {
        let transaction = db
            .driver
            .transaction(&db.database, TransactionType::Read)
            .await?;
        let rows = transaction
            .query_with_rows(query, rows)
            .await?
            .into_rows()
            .try_collect()
            .await?;
        transaction.close().await?;
        Ok(rows)
    }

    fn ids(rows: &[ConceptRow]) -> Result<Vec<i64>> {
        rows.iter()
            .map(|row| {
                row.get("id")?
                    .and_then(|concept| concept.try_get_integer())
                    .context("integer id")
            })
            .collect()
    }

    async fn tags(db: &TypeDb, todo_id: i64) -> Result<Vec<i64>> {
        ids(&read(
            db,
            "\
            given $tid: integer; \
            match $t isa todo, has todo-id == $tid; \
                (todo: $t, tag: $g) isa todo-tag; $g has tag-id $id; \
            select $id; sort $id;",
            given(&["tid"], vec![todo_id.into()])?,
        )
        .await?)
    }

    async fn check_operations(db: &TypeDb, fixture: &FixtureConfig) -> Result<()> {
        db.reset_and_seed(fixture).await?;
        let listed = read(db, LIST_TODOS, given(&["uid"], vec![1.into()])?).await?;
        let expected: Vec<_> = (1_i64..=32)
            .rev()
            .filter(|id| (id - 1) % 3 != 0)
            .take(20)
            .collect();
        ensure!(
            ids(&listed)? == expected,
            "todo read must filter, sort, and limit within one user"
        );
        for row in &listed {
            let id = row.get("id")?.unwrap().try_get_integer().unwrap();
            ensure!(
                row.get("title")?.unwrap().try_get_string()
                    == Some(format!("Todo {id} for user 1").as_str())
            );
            ensure!(row.get("created")?.unwrap().try_get_integer() == Some(1_700_000_000 + id));
        }

        let joined = read(
            db,
            LIST_BY_TAGS,
            given(&["uid", "a", "b"], vec![1.into(), 1.into(), 2.into()])?,
        )
        .await?;
        let expected: Vec<_> = (1_i64..=32)
            .rev()
            .filter(|id| {
                let tags = fixture.tag_ids_for_todo(1, *id);
                (id - 1) % 3 != 0 && tags.contains(&1) && tags.contains(&2)
            })
            .collect();
        ensure!(
            ids(&joined)? == expected,
            "tag read must require both tags without duplicates"
        );
        let same_tag = read(
            db,
            LIST_BY_TAGS,
            given(&["uid", "a", "b"], vec![1.into(), 1.into(), 1.into()])?,
        )
        .await?;
        ensure!(same_tag.is_empty(), "tag read requires two distinct tags");

        db.execute(&Operation::Toggle {
            todo_id: 32,
            completed: true,
            updated_at: 2_000_000_001,
        })
        .await?;
        let toggled = read(
            db,
            "\
            given $tid: integer; \
            match $t isa todo, has todo-id == $tid, has completed $done, has updated-at $updated; \
            select $done, $updated;",
            given(&["tid"], vec![32.into()])?,
        )
        .await?;
        ensure!(toggled.len() == 1);
        ensure!(toggled[0].get("done")?.unwrap().try_get_boolean() == Some(true));
        ensure!(toggled[0].get("updated")?.unwrap().try_get_integer() == Some(2_000_000_001));

        let title = "Quoted \"todo\", backslash \\, and newline\n";
        db.execute(&Operation::InsertTagged {
            todo_id: 65,
            user_id: 1,
            title: title.into(),
            timestamp: 2_000_000_002,
            tag_ids: vec![1, 2, 3],
        })
        .await?;
        let listed = read(db, LIST_TODOS, given(&["uid"], vec![1.into()])?).await?;
        ensure!(ids(&listed)?[0] == 65);
        ensure!(listed[0].get("title")?.unwrap().try_get_string() == Some(title));
        ensure!(tags(db, 65).await? == [1, 2, 3]);

        for tag_ids in [vec![2, 3, 4], vec![2, 3, 4], vec![], vec![1, 2]] {
            db.execute(&Operation::Retag {
                todo_id: 65,
                user_id: 1,
                tag_ids: tag_ids.clone(),
            })
            .await?;
            ensure!(
                tags(db, 65).await? == tag_ids,
                "retag must replace the whole tag set"
            );
        }
        ensure!(
            db.execute(&Operation::Retag {
                todo_id: 65,
                user_id: 1,
                tag_ids: vec![2, 5]
            })
            .await
            .is_err()
        );
        ensure!(
            tags(db, 65).await? == [1, 2],
            "failed retag must roll back deletions and inserts"
        );
        for (todo_id, user_id, tag_ids) in [
            (66, 1, vec![1, 5]),
            (66, 3, vec![1, 2]),
            (65, 1, vec![1, 2]),
        ] {
            ensure!(
                db.execute(&Operation::InsertTagged {
                    todo_id,
                    user_id,
                    title: "invalid".into(),
                    timestamp: 0,
                    tag_ids,
                })
                .await
                .is_err(),
                "invalid owner, tag, or duplicate key must fail"
            );
        }
        let missing = read(
            db,
            "\
            given $tid: integer; match $t isa todo, has todo-id == $tid; select $t;",
            given(&["tid"], vec![66.into()])?,
        )
        .await?;
        ensure!(missing.is_empty(), "failed insert must roll back its todo");

        db.reset_and_seed(fixture).await?;
        let rows = read(
            db,
            "given $min: integer; match $t isa todo, has todo-id $id; $id >= $min; select $id;",
            given(&["min"], vec![1.into()])?,
        )
        .await?;
        ensure!(
            rows.len() == fixture.seed_todo_count() as usize,
            "reset must discard previous writes"
        );
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires local TypeDB 3.14 with credentials from TYPEDB_* environment variables"]
    async fn typedb_roundtrip() -> Result<()> {
        let mut config = crate::Cli::try_parse_from(["benchmark"])?.typedb;
        config.database = format!("rust-todo-benchmark-test-{}", std::process::id());
        let db = TypeDb::connect(&config).await?;
        let fixture = FixtureConfig {
            users: 2,
            todos_per_user: 32,
            tags_per_user: 4,
            tags_per_todo: 2,
        };
        let result = async {
            check_operations(&db, &fixture).await?;
            let var_dir =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("var/typedb-checks");
            let mut backend = crate::backend::Backend::open(
                crate::model::BackendKind::Typedb,
                &var_dir,
                55432,
                crate::model::Durability::Durable,
                &config,
            )
            .await?;
            for workload in crate::model::Workload::ALL {
                for concurrency in [1, 8] {
                    backend.reset_and_seed(&fixture).await?;
                    let result = crate::benchmark::run_case(
                        &backend,
                        &fixture,
                        workload,
                        concurrency,
                        std::time::Duration::ZERO,
                        std::time::Duration::from_millis(250),
                        Arc::new(std::sync::atomic::AtomicU64::new(
                            fixture.seed_todo_count() + 1,
                        )),
                    )
                    .await?;
                    eprintln!(
                        "TypeDB {workload}/{concurrency}: {} successful, {} failed, {:?}",
                        result.successful_operations, result.failed_operations, result.first_error
                    );
                    ensure!(result.successful_operations > 0);
                    if concurrency == 1 {
                        ensure!(result.failed_operations == 0, "{:?}", result.first_error);
                    } else if let Some(error) = &result.first_error {
                        ensure!(error.to_lowercase().contains("conflict"), "{error}");
                    }
                    ensure!(serde_json::to_value(&result)?["backend"] == "typedb");
                }
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        let cleanup = async {
            db.driver
                .databases()
                .get(&db.database)
                .await?
                .delete()
                .await
        }
        .await;
        result?;
        cleanup?;
        Ok(())
    }
}
