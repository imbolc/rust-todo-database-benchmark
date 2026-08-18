pub const POSTGRES_SCHEMA: &str = r#"
DROP TABLE IF EXISTS todo_tags;
DROP TABLE IF EXISTS tags;
DROP TABLE IF EXISTS todos;
DROP TABLE IF EXISTS users;

CREATE TABLE users (
    id          BIGINT PRIMARY KEY,
    email       TEXT NOT NULL UNIQUE,
    created_at  BIGINT NOT NULL
);

CREATE TABLE todos (
    id          BIGINT PRIMARY KEY,
    user_id     BIGINT NOT NULL REFERENCES users(id),
    title       TEXT NOT NULL,
    completed   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  BIGINT NOT NULL,
    updated_at  BIGINT NOT NULL,
    UNIQUE (user_id, id)
);

CREATE INDEX todos_user_status_created
    ON todos(user_id, completed, created_at DESC);

CREATE TABLE tags (
    id          BIGINT PRIMARY KEY,
    user_id     BIGINT NOT NULL REFERENCES users(id),
    name        TEXT NOT NULL,
    UNIQUE (user_id, name),
    UNIQUE (user_id, id)
);

CREATE TABLE todo_tags (
    user_id     BIGINT NOT NULL,
    todo_id     BIGINT NOT NULL,
    tag_id      BIGINT NOT NULL,
    PRIMARY KEY (todo_id, tag_id),
    FOREIGN KEY (user_id, todo_id)
        REFERENCES todos(user_id, id) ON DELETE CASCADE,
    FOREIGN KEY (user_id, tag_id)
        REFERENCES tags(user_id, id) ON DELETE CASCADE
);

CREATE INDEX todo_tags_by_tag
    ON todo_tags(tag_id, todo_id);
"#;

pub const SQLITE_SCHEMA: &str = r#"
PRAGMA foreign_keys = OFF;
DROP TABLE IF EXISTS todo_tags;
DROP TABLE IF EXISTS tags;
DROP TABLE IF EXISTS todos;
DROP TABLE IF EXISTS users;
PRAGMA foreign_keys = ON;

CREATE TABLE users (
    id          INTEGER PRIMARY KEY,
    email       TEXT NOT NULL UNIQUE,
    created_at  INTEGER NOT NULL
);

CREATE TABLE todos (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    title       TEXT NOT NULL,
    completed   INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    UNIQUE (user_id, id)
);

CREATE INDEX todos_user_status_created
    ON todos(user_id, completed, created_at DESC);

CREATE TABLE tags (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    name        TEXT NOT NULL,
    UNIQUE (user_id, name),
    UNIQUE (user_id, id)
);

CREATE TABLE todo_tags (
    user_id     INTEGER NOT NULL,
    todo_id     INTEGER NOT NULL,
    tag_id      INTEGER NOT NULL,
    PRIMARY KEY (todo_id, tag_id),
    FOREIGN KEY (user_id, todo_id)
        REFERENCES todos(user_id, id) ON DELETE CASCADE,
    FOREIGN KEY (user_id, tag_id)
        REFERENCES tags(user_id, id) ON DELETE CASCADE
);

CREATE INDEX todo_tags_by_tag
    ON todo_tags(tag_id, todo_id);
"#;
