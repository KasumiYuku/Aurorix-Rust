use std::collections::{BTreeMap, BTreeSet};

use sqlx::PgPool;

use crate::error::DbResult;
use crate::plugin_db::PLUGIN_MIGRATIONS_TABLE;
use crate::schema::{self, ColumnSpec, TableSpec};

pub const COLUMNS_QUERY: &str = "SELECT table_name, column_name, data_type, is_nullable\n\t\tFROM information_schema.columns\n\t\tWHERE table_schema = current_schema() AND table_name = ANY($1)\n\t\tORDER BY table_name, ordinal_position";

pub const INDEXES_QUERY: &str = "SELECT tablename, indexname, indexdef FROM pg_indexes\n\t\tWHERE schemaname = current_schema() AND tablename = ANY($1)";

pub const TABLES_QUERY: &str = "SELECT table_name FROM information_schema.tables\n\t\tWHERE table_schema = current_schema() AND table_type = 'BASE TABLE'\n\t\tORDER BY table_name";

pub const COUNT_QUERY: &str = "SELECT count(*) FROM ";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeMismatch {
    pub column: String,
    pub expected_type: String,
    pub actual_type: String,
    pub expected_nullable: bool,
    pub actual_nullable: bool,
}

impl TypeMismatch {
    pub fn describe(&self) -> String {
        format!(
            "{}(期望 {}{} 实际 {}{})",
            self.column,
            self.expected_type,
            if self.expected_nullable {
                " 可空"
            } else {
                " 非空"
            },
            self.actual_type,
            if self.actual_nullable {
                " 可空"
            } else {
                " 非空"
            }
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TableReport {
    pub table: String,
    pub present: bool,
    pub missing_columns: Vec<String>,
    pub unexpected_columns: Vec<String>,
    pub type_mismatches: Vec<TypeMismatch>,
    pub missing_unique_key: bool,
    pub missing_indexes: Vec<String>,
    pub row_count: Option<i64>,
    pub compatible: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaReport {
    pub tables: Vec<TableReport>,
    pub legacy_tables: Vec<String>,
    pub compatible: bool,
    pub summary: String,
}

impl SchemaReport {
    pub fn table(&self, name: &str) -> Option<&TableReport> {
        self.tables.iter().find(|report| report.table == name)
    }

    pub fn incompatible_tables(&self) -> Vec<&TableReport> {
        self.tables
            .iter()
            .filter(|report| !report.compatible)
            .collect()
    }

    pub fn compatible_count(&self) -> usize {
        self.tables
            .iter()
            .filter(|report| report.compatible)
            .count()
    }
}

#[derive(Debug, Clone)]
struct ColumnFact {
    data_type: String,
    nullable: bool,
}

#[derive(Debug, Clone, Default)]
struct IndexFacts {
    names: BTreeSet<String>,
    unique_keys: Vec<Vec<String>>,
}

pub async fn verify_existing_schema(pool: &PgPool) -> DbResult<SchemaReport> {
    let names: Vec<String> = schema::CORE_TABLE_NAMES
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let tables: Vec<String> = sqlx::query_scalar(TABLES_QUERY).fetch_all(pool).await?;
    let column_rows: Vec<(String, String, String, String)> = sqlx::query_as(COLUMNS_QUERY)
        .bind(&names)
        .fetch_all(pool)
        .await?;
    let index_rows: Vec<(String, String, String)> = sqlx::query_as(INDEXES_QUERY)
        .bind(&names)
        .fetch_all(pool)
        .await?;

    let present_tables: BTreeSet<&str> = tables.iter().map(String::as_str).collect();
    let mut columns: BTreeMap<String, BTreeMap<String, ColumnFact>> = BTreeMap::new();
    for (table, column, data_type, nullable) in column_rows {
        columns.entry(table).or_default().insert(
            column,
            ColumnFact {
                data_type,
                nullable: nullable == "YES",
            },
        );
    }
    let mut indexes: BTreeMap<String, IndexFacts> = BTreeMap::new();
    for (table, index_name, index_def) in index_rows {
        let entry = indexes.entry(table).or_default();
        entry.names.insert(index_name);
        if index_def.contains("UNIQUE") {
            entry.unique_keys.push(index_columns(&index_def));
        }
    }

    let mut reports = Vec::with_capacity(schema::CORE_TABLES.len());
    for spec in schema::CORE_TABLES {
        let present = present_tables.contains(spec.name);
        let row_count = if present {
            count_rows(pool, spec.name).await?
        } else {
            None
        };
        let empty = BTreeMap::new();
        let actual = columns.get(spec.name).unwrap_or(&empty);
        let facts = indexes.get(spec.name).cloned().unwrap_or_default();
        reports.push(build_report(spec, present, actual, &facts, row_count));
    }

    let core_names: BTreeSet<&str> = schema::CORE_TABLE_NAMES.into_iter().collect();
    let legacy_tables: Vec<String> = tables
        .into_iter()
        .filter(|name| !core_names.contains(name.as_str()))
        .filter(|name| name != PLUGIN_MIGRATIONS_TABLE)
        .collect();

    let compatible = reports.iter().all(|report| report.compatible);
    let compatible_count = reports.iter().filter(|report| report.compatible).count();
    let summary = if compatible {
        format!(
            "核心表 {} 张全部与预期一致, 遗留表 {} 张",
            reports.len(),
            legacy_tables.len()
        )
    } else {
        format!(
            "核心表 {} 张中 {} 张兼容, {} 张需要人工核对; 遗留表 {} 张",
            reports.len(),
            compatible_count,
            reports.len() - compatible_count,
            legacy_tables.len()
        )
    };
    Ok(SchemaReport {
        tables: reports,
        legacy_tables,
        compatible,
        summary,
    })
}

async fn count_rows(pool: &PgPool, table: &'static str) -> DbResult<Option<i64>> {
    let statement = format!("{COUNT_QUERY}{table}");
    let total: i64 = sqlx::query_scalar(&statement).fetch_one(pool).await?;
    Ok(Some(total))
}

fn index_columns(index_def: &str) -> Vec<String> {
    let Some(open) = index_def.find('(') else {
        return Vec::new();
    };
    let Some(close) = index_def.rfind(')') else {
        return Vec::new();
    };
    if close <= open {
        return Vec::new();
    }
    index_def[open + 1..close]
        .split(',')
        .map(|part| {
            part.trim()
                .trim_end_matches(" DESC")
                .trim_end_matches(" ASC")
                .trim()
                .to_string()
        })
        .filter(|part| !part.is_empty())
        .collect()
}

fn build_report(
    spec: &TableSpec,
    present: bool,
    actual: &BTreeMap<String, ColumnFact>,
    facts: &IndexFacts,
    row_count: Option<i64>,
) -> TableReport {
    let expected_names: BTreeSet<&str> = spec.columns.iter().map(|column| column.name).collect();
    let mut missing_columns = Vec::new();
    let mut type_mismatches = Vec::new();
    if present {
        for column in spec.columns {
            match actual.get(column.name) {
                None => missing_columns.push(column.name.to_string()),
                Some(fact) => {
                    if fact.data_type != column.data_type || fact.nullable != column.nullable {
                        type_mismatches.push(mismatch_of(column, fact));
                    }
                }
            }
        }
    }
    let unexpected_columns: Vec<String> = if present {
        actual
            .keys()
            .filter(|name| !expected_names.contains(name.as_str()))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let missing_unique_key = present
        && !facts
            .unique_keys
            .iter()
            .any(|columns| covers(columns, spec.unique_key));
    let missing_indexes: Vec<String> = if present {
        spec.indexes
            .iter()
            .filter(|name| !facts.names.contains(**name))
            .map(|name| (*name).to_string())
            .collect()
    } else {
        Vec::new()
    };
    let compatible = present
        && missing_columns.is_empty()
        && unexpected_columns.is_empty()
        && type_mismatches.is_empty()
        && !missing_unique_key;
    let mut report = TableReport {
        table: spec.name.to_string(),
        present,
        missing_columns,
        unexpected_columns,
        type_mismatches,
        missing_unique_key,
        missing_indexes,
        row_count,
        compatible,
        summary: String::new(),
    };
    report.summary = summarize(spec, &report);
    report
}

fn mismatch_of(column: &ColumnSpec, fact: &ColumnFact) -> TypeMismatch {
    TypeMismatch {
        column: column.name.to_string(),
        expected_type: column.data_type.to_string(),
        actual_type: fact.data_type.clone(),
        expected_nullable: column.nullable,
        actual_nullable: fact.nullable,
    }
}

fn covers(index_columns: &[String], key: &[&str]) -> bool {
    key.iter()
        .all(|column| index_columns.iter().any(|actual| actual == column))
}

fn summarize(spec: &TableSpec, report: &TableReport) -> String {
    if !report.present {
        return format!("{}: 表不存在, 不兼容", spec.name);
    }
    let rows = report.row_count.unwrap_or(0);
    let mut problems = Vec::new();
    if !report.missing_columns.is_empty() {
        problems.push(format!("缺少列 {}", report.missing_columns.join(", ")));
    }
    if !report.unexpected_columns.is_empty() {
        problems.push(format!("多出列 {}", report.unexpected_columns.join(", ")));
    }
    if !report.type_mismatches.is_empty() {
        let described: Vec<String> = report
            .type_mismatches
            .iter()
            .map(TypeMismatch::describe)
            .collect();
        problems.push(format!("列定义不符 {}", described.join(", ")));
    }
    if report.missing_unique_key {
        problems.push(format!("缺少唯一键 ({})", spec.unique_key.join(", ")));
    }
    if !report.compatible {
        return format!(
            "{}: {}; 共 {rows} 行, 不兼容",
            spec.name,
            problems.join("; ")
        );
    }
    let mut summary = format!(
        "{}: {} 列齐全, 共 {rows} 行, 兼容",
        spec.name,
        spec.columns.len()
    );
    if !report.missing_indexes.is_empty() {
        summary.push_str(&format!(
            "; 建议补建索引 {}",
            report.missing_indexes.join(", ")
        ));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::BIGINT;
    use crate::schema::TEXT;

    fn facts(columns: &[(&str, &str, bool)]) -> BTreeMap<String, ColumnFact> {
        columns
            .iter()
            .map(|(name, data_type, nullable)| {
                (
                    (*name).to_string(),
                    ColumnFact {
                        data_type: (*data_type).to_string(),
                        nullable: *nullable,
                    },
                )
            })
            .collect()
    }

    fn kv_facts() -> BTreeMap<String, ColumnFact> {
        facts(&[
            ("scope", TEXT, false),
            ("namespace", TEXT, false),
            ("key", TEXT, false),
            ("value", "bytea", false),
            ("updated_at", "timestamp with time zone", false),
        ])
    }

    fn kv_indexes() -> IndexFacts {
        IndexFacts {
            names: BTreeSet::new(),
            unique_keys: vec![vec![
                "scope".to_string(),
                "namespace".to_string(),
                "key".to_string(),
            ]],
        }
    }

    fn kv_spec() -> &'static TableSpec {
        schema::core_table("kv_data").expect("kv_data 预期")
    }

    #[test]
    fn matching_columns_are_compatible() {
        let report = build_report(kv_spec(), true, &kv_facts(), &kv_indexes(), Some(12));
        assert!(report.compatible);
        assert_eq!(report.summary, "kv_data: 5 列齐全, 共 12 行, 兼容");
        assert!(report.missing_columns.is_empty());
        assert!(report.type_mismatches.is_empty());
    }

    #[test]
    fn missing_column_is_reported_and_incompatible() {
        let mut columns = kv_facts();
        columns.remove("updated_at");
        let report = build_report(kv_spec(), true, &columns, &kv_indexes(), Some(0));
        assert!(!report.compatible);
        assert_eq!(report.missing_columns, vec!["updated_at".to_string()]);
        assert!(
            report.summary.contains("缺少列 updated_at"),
            "{}",
            report.summary
        );
        assert!(report.summary.ends_with("不兼容"));
    }

    #[test]
    fn extra_column_is_reported_and_incompatible() {
        let mut columns = kv_facts();
        columns.insert(
            "legacy_note".to_string(),
            ColumnFact {
                data_type: TEXT.to_string(),
                nullable: true,
            },
        );
        let report = build_report(kv_spec(), true, &columns, &kv_indexes(), Some(3));
        assert!(!report.compatible);
        assert_eq!(report.unexpected_columns, vec!["legacy_note".to_string()]);
        assert!(report.summary.contains("多出列 legacy_note"));
    }

    #[test]
    fn type_and_nullability_drift_is_reported() {
        let mut columns = kv_facts();
        columns.insert(
            "value".to_string(),
            ColumnFact {
                data_type: TEXT.to_string(),
                nullable: true,
            },
        );
        let report = build_report(kv_spec(), true, &columns, &kv_indexes(), Some(1));
        assert!(!report.compatible);
        assert_eq!(report.type_mismatches.len(), 1);
        let mismatch = &report.type_mismatches[0];
        assert_eq!(mismatch.column, "value");
        assert_eq!(mismatch.expected_type, "bytea");
        assert_eq!(mismatch.actual_type, TEXT);
        assert!(!mismatch.expected_nullable);
        assert!(mismatch.actual_nullable);
        assert!(report.summary.contains("列定义不符 value"));
    }

    #[test]
    fn missing_unique_key_is_reported() {
        let facts = IndexFacts::default();
        let report = build_report(kv_spec(), true, &kv_facts(), &facts, Some(0));
        assert!(!report.compatible);
        assert!(report.missing_unique_key);
        assert!(report.summary.contains("缺少唯一键"));
    }

    #[test]
    fn absent_table_is_reported() {
        let report = build_report(
            kv_spec(),
            false,
            &BTreeMap::new(),
            &IndexFacts::default(),
            None,
        );
        assert!(!report.compatible);
        assert_eq!(report.summary, "kv_data: 表不存在, 不兼容");
        assert_eq!(report.row_count, None);
    }

    #[test]
    fn advisory_indexes_do_not_break_compatibility() {
        let spec = schema::core_table("inbox_messages").expect("inbox_messages 预期");
        let columns = facts(&[
            ("id", BIGINT, false),
            ("conv_id", TEXT, false),
            ("direction", TEXT, false),
            ("sender_id", TEXT, false),
            ("sender_name", TEXT, false),
            ("content", TEXT, false),
            ("platform_msg_id", TEXT, false),
            ("created_at", BIGINT, false),
        ]);
        let indexes = IndexFacts {
            names: BTreeSet::new(),
            unique_keys: vec![vec!["id".to_string()]],
        };
        let report = build_report(spec, true, &columns, &indexes, Some(7));
        assert!(report.compatible, "{}", report.summary);
        assert_eq!(
            report.missing_indexes,
            vec![
                "inbox_messages_conv".to_string(),
                "inbox_messages_at".to_string()
            ]
        );
        assert!(report.summary.contains("建议补建索引"));
    }

    #[test]
    fn index_columns_parser_handles_sort_and_desc() {
        assert_eq!(
            index_columns("CREATE UNIQUE INDEX kv_data_pkey ON public.kv_data USING btree (scope, namespace, key)"),
            vec!["scope".to_string(), "namespace".to_string(), "key".to_string()]
        );
        assert_eq!(
            index_columns("CREATE INDEX stats_conversations_msg ON public.stats_conversations USING btree (msg_count DESC)"),
            vec!["msg_count".to_string()]
        );
        assert!(index_columns("garbage").is_empty());
    }

    #[test]
    fn coverage_requires_every_key_column() {
        let columns = vec!["scope".to_string(), "namespace".to_string()];
        assert!(!covers(&columns, &["scope", "namespace", "key"]));
        let full = vec![
            "scope".to_string(),
            "namespace".to_string(),
            "key".to_string(),
        ];
        assert!(covers(&full, &["scope", "namespace", "key"]));
    }
}
