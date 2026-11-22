use sqlx::PgPool;

use crate::error::{DbError, DbResult};

pub const KV_DATA: &str = "CREATE TABLE IF NOT EXISTS kv_data (\n\tscope      TEXT NOT NULL,\n\tnamespace  TEXT NOT NULL,\n\tkey        TEXT NOT NULL,\n\tvalue      BYTEA NOT NULL,\n\tupdated_at TIMESTAMPTZ NOT NULL DEFAULT now(),\n\tPRIMARY KEY (scope, namespace, key)\n)";

pub const STATS_CONVERSATIONS: &str = "CREATE TABLE IF NOT EXISTS stats_conversations (\n\tscene TEXT NOT NULL,\n\tid TEXT NOT NULL,\n\tname TEXT NOT NULL DEFAULT '',\n\tmsg_count BIGINT NOT NULL DEFAULT 0,\n\tfriend BOOLEAN NOT NULL DEFAULT false,\n\tPRIMARY KEY (scene, id)\n)";

pub const STATS_CONVERSATIONS_FRIEND: &str =
    "ALTER TABLE stats_conversations ADD COLUMN IF NOT EXISTS friend BOOLEAN NOT NULL DEFAULT false";

pub const STATS_DAILY: &str = "CREATE TABLE IF NOT EXISTS stats_daily (\n\tday TEXT PRIMARY KEY,\n\trecv BIGINT NOT NULL DEFAULT 0,\n\tsent BIGINT NOT NULL DEFAULT 0,\n\tbutton BIGINT NOT NULL DEFAULT 0\n)";

pub const STATS_MIGRATIONS: &str = "CREATE TABLE IF NOT EXISTS stats_migrations (\n\tname TEXT PRIMARY KEY,\n\tat TIMESTAMPTZ NOT NULL DEFAULT now()\n)";

pub const STATS_CONVERSATIONS_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS stats_conversations_msg ON stats_conversations(msg_count DESC)";

pub const INBOX_CONVERSATIONS: &str = "CREATE TABLE IF NOT EXISTS inbox_conversations (\n\tid           TEXT PRIMARY KEY,\n\tkind         TEXT NOT NULL,\n\tname         TEXT NOT NULL DEFAULT '',\n\tlast_at      BIGINT NOT NULL DEFAULT 0,\n\tlast_preview TEXT NOT NULL DEFAULT ''\n)";

pub const INBOX_MESSAGES: &str = "CREATE TABLE IF NOT EXISTS inbox_messages (\n\tid              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\n\tconv_id         TEXT NOT NULL,\n\tdirection       TEXT NOT NULL,\n\tsender_id       TEXT NOT NULL DEFAULT '',\n\tsender_name     TEXT NOT NULL DEFAULT '',\n\tcontent         TEXT NOT NULL DEFAULT '',\n\tplatform_msg_id TEXT NOT NULL DEFAULT '',\n\tcreated_at      BIGINT NOT NULL\n)";

pub const INBOX_MESSAGES_CONV_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS inbox_messages_conv ON inbox_messages(conv_id, id)";

pub const INBOX_MESSAGES_AT_INDEX: &str =
    "CREATE INDEX IF NOT EXISTS inbox_messages_at ON inbox_messages(created_at)";

pub const STATEMENTS: [&str; 10] = [
    KV_DATA,
    STATS_CONVERSATIONS,
    STATS_CONVERSATIONS_FRIEND,
    STATS_DAILY,
    STATS_MIGRATIONS,
    STATS_CONVERSATIONS_INDEX,
    INBOX_CONVERSATIONS,
    INBOX_MESSAGES,
    INBOX_MESSAGES_CONV_INDEX,
    INBOX_MESSAGES_AT_INDEX,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnSpec {
    pub name: &'static str,
    pub data_type: &'static str,
    pub nullable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableSpec {
    pub name: &'static str,
    pub columns: &'static [ColumnSpec],
    pub unique_key: &'static [&'static str],
    pub indexes: &'static [&'static str],
}

pub const TEXT: &str = "text";
pub const BIGINT: &str = "bigint";
pub const BOOLEAN: &str = "boolean";
pub const BYTEA: &str = "bytea";
pub const TIMESTAMPTZ: &str = "timestamp with time zone";

pub const KV_DATA_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        name: "scope",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "namespace",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "key",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "value",
        data_type: BYTEA,
        nullable: false,
    },
    ColumnSpec {
        name: "updated_at",
        data_type: TIMESTAMPTZ,
        nullable: false,
    },
];

pub const STATS_CONVERSATIONS_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        name: "scene",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "id",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "name",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "msg_count",
        data_type: BIGINT,
        nullable: false,
    },
    ColumnSpec {
        name: "friend",
        data_type: BOOLEAN,
        nullable: false,
    },
];

pub const STATS_DAILY_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        name: "day",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "recv",
        data_type: BIGINT,
        nullable: false,
    },
    ColumnSpec {
        name: "sent",
        data_type: BIGINT,
        nullable: false,
    },
    ColumnSpec {
        name: "button",
        data_type: BIGINT,
        nullable: false,
    },
];

pub const STATS_MIGRATIONS_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        name: "name",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "at",
        data_type: TIMESTAMPTZ,
        nullable: false,
    },
];

pub const INBOX_CONVERSATIONS_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        name: "id",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "kind",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "name",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "last_at",
        data_type: BIGINT,
        nullable: false,
    },
    ColumnSpec {
        name: "last_preview",
        data_type: TEXT,
        nullable: false,
    },
];

pub const INBOX_MESSAGES_COLUMNS: &[ColumnSpec] = &[
    ColumnSpec {
        name: "id",
        data_type: BIGINT,
        nullable: false,
    },
    ColumnSpec {
        name: "conv_id",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "direction",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "sender_id",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "sender_name",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "content",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "platform_msg_id",
        data_type: TEXT,
        nullable: false,
    },
    ColumnSpec {
        name: "created_at",
        data_type: BIGINT,
        nullable: false,
    },
];

pub const CORE_TABLES: &[TableSpec] = &[
    TableSpec {
        name: "kv_data",
        columns: KV_DATA_COLUMNS,
        unique_key: &["scope", "namespace", "key"],
        indexes: &[],
    },
    TableSpec {
        name: "stats_conversations",
        columns: STATS_CONVERSATIONS_COLUMNS,
        unique_key: &["scene", "id"],
        indexes: &["stats_conversations_msg"],
    },
    TableSpec {
        name: "stats_daily",
        columns: STATS_DAILY_COLUMNS,
        unique_key: &["day"],
        indexes: &[],
    },
    TableSpec {
        name: "stats_migrations",
        columns: STATS_MIGRATIONS_COLUMNS,
        unique_key: &["name"],
        indexes: &[],
    },
    TableSpec {
        name: "inbox_conversations",
        columns: INBOX_CONVERSATIONS_COLUMNS,
        unique_key: &["id"],
        indexes: &[],
    },
    TableSpec {
        name: "inbox_messages",
        columns: INBOX_MESSAGES_COLUMNS,
        unique_key: &["id"],
        indexes: &["inbox_messages_conv", "inbox_messages_at"],
    },
];

pub fn core_table(name: &str) -> Option<&'static TableSpec> {
    CORE_TABLES.iter().find(|spec| spec.name == name)
}

pub const CORE_TABLE_NAMES: [&str; 6] = [
    "kv_data",
    "stats_conversations",
    "stats_daily",
    "stats_migrations",
    "inbox_conversations",
    "inbox_messages",
];

pub async fn ensure_schema(pool: &PgPool) -> DbResult<()> {
    for statement in STATEMENTS {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|err| DbError::Schema(format!("{err}: {statement}")))?;
    }
    Ok(())
}

pub async fn table_exists(pool: &PgPool, table: &str) -> DbResult<bool> {
    let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(table)
        .fetch_one(pool)
        .await?;
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expectations_cover_exactly_the_six_core_tables() {
        assert_eq!(CORE_TABLES.len(), 6);
        let names: Vec<&str> = CORE_TABLES.iter().map(|spec| spec.name).collect();
        assert_eq!(names, CORE_TABLE_NAMES.to_vec());
        for name in CORE_TABLE_NAMES {
            assert!(core_table(name).is_some(), "{name} 缺少预期定义");
        }
    }

    fn ddl_of(table: &str) -> &'static str {
        match table {
            "kv_data" => KV_DATA,
            "stats_conversations" => STATS_CONVERSATIONS,
            "stats_daily" => STATS_DAILY,
            "stats_migrations" => STATS_MIGRATIONS,
            "inbox_conversations" => INBOX_CONVERSATIONS,
            "inbox_messages" => INBOX_MESSAGES,
            other => panic!("未知核心表 {other}"),
        }
    }

    fn ddl_keyword(data_type: &str) -> &'static str {
        match data_type {
            TEXT => "text",
            BIGINT => "bigint",
            BOOLEAN => "boolean",
            BYTEA => "bytea",
            TIMESTAMPTZ => "timestamptz",
            _ => "timestamp with time zone",
        }
    }

    #[test]
    fn every_expected_column_exists_in_the_ddl() {
        for spec in CORE_TABLES {
            let ddl = ddl_of(spec.name).to_lowercase();
            for column in spec.columns {
                assert!(
                    ddl.contains(column.name),
                    "{} 的 DDL 缺少列 {}",
                    spec.name,
                    column.name
                );
                assert!(
                    ddl.contains(ddl_keyword(column.data_type)),
                    "{} DDL 未声明类型 {}",
                    spec.name,
                    column.data_type
                );
            }
            for key in spec.unique_key {
                assert!(ddl.contains(key), "{} 的 DDL 缺少键列 {}", spec.name, key);
            }
        }
    }

    #[test]
    fn expected_column_counts_match_the_ddl_columns() {
        assert_eq!(KV_DATA_COLUMNS.len(), 5);
        assert_eq!(STATS_CONVERSATIONS_COLUMNS.len(), 5);
        assert_eq!(STATS_DAILY_COLUMNS.len(), 4);
        assert_eq!(STATS_MIGRATIONS_COLUMNS.len(), 2);
        assert_eq!(INBOX_CONVERSATIONS_COLUMNS.len(), 5);
        assert_eq!(INBOX_MESSAGES_COLUMNS.len(), 8);
        let index_names: Vec<&str> = CORE_TABLES
            .iter()
            .flat_map(|spec| spec.indexes.iter().copied())
            .collect();
        assert_eq!(
            index_names,
            vec![
                "stats_conversations_msg",
                "inbox_messages_conv",
                "inbox_messages_at"
            ]
        );
    }
}
