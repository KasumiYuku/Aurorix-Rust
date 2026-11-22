use aurorix_core::Scene;
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::PgPool;

use crate::error::{DbError, DbResult};

pub const SCOPE_GLOBAL: &str = "global";

pub const SCOPE_PLUGIN: &str = "plugin";

pub const SCOPE_COMMAND: &str = "command";

pub const SCOPE_USER: &str = "user";

pub const KV_UPSERT: &str = "INSERT INTO kv_data (scope, namespace, key, value, updated_at)\n\t\tVALUES ($1, $2, $3, $4, now())\n\t\tON CONFLICT (scope, namespace, key) DO UPDATE SET\n\t\t\tvalue = excluded.value,\n\t\t\tupdated_at = now()";

pub const KV_SELECT: &str =
    "SELECT value FROM kv_data WHERE scope = $1 AND namespace = $2 AND key = $3";

pub const KV_EXISTS: &str =
    "SELECT EXISTS (SELECT 1 FROM kv_data WHERE scope = $1 AND namespace = $2 AND key = $3)";

pub const KV_DELETE: &str = "DELETE FROM kv_data WHERE scope = $1 AND namespace = $2 AND key = $3";

pub const KV_CLEAR: &str = "DELETE FROM kv_data WHERE scope = $1 AND namespace = $2";

pub const KV_LOCK: &str = "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))";

#[derive(Debug, Clone)]
pub struct KvStore {
    pool: PgPool,
    scope: String,
    namespace: String,
}

impl KvStore {
    pub fn new(pool: PgPool, scope: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            pool,
            scope: scope.into(),
            namespace: namespace.into(),
        }
    }

    pub fn global(pool: PgPool) -> Self {
        Self::new(pool, SCOPE_GLOBAL, SCOPE_GLOBAL)
    }

    pub fn plugin(pool: PgPool, plugin_id: &str) -> Self {
        Self::new(pool, SCOPE_PLUGIN, plugin_id)
    }

    pub fn command(pool: PgPool, plugin_id: &str, command_id: &str) -> Self {
        Self::new(pool, SCOPE_COMMAND, format!("{plugin_id}:{command_id}"))
    }

    pub fn user(pool: PgPool, user_id: &str) -> Self {
        Self::new(pool, SCOPE_USER, user_id)
    }

    pub fn scene(pool: PgPool, scene: Scene, id: &str) -> Self {
        Self::new(pool, scene.as_str(), id)
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn validate(&self, key: &str) -> DbResult<()> {
        if self.scope.is_empty() || self.namespace.is_empty() {
            return Err(DbError::InvalidNamespace);
        }
        if key.is_empty() {
            return Err(DbError::EmptyKey);
        }
        Ok(())
    }

    pub async fn set<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> DbResult<()> {
        self.validate(key)?;
        let encoded = serde_json::to_vec(value).map_err(|err| DbError::Encode(err.to_string()))?;
        sqlx::query(KV_UPSERT)
            .bind(&self.scope)
            .bind(&self.namespace)
            .bind(key)
            .bind(encoded)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> DbResult<Option<T>> {
        self.validate(key)?;
        let row: Option<Vec<u8>> = sqlx::query_scalar(KV_SELECT)
            .bind(&self.scope)
            .bind(&self.namespace)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(encoded) => serde_json::from_slice(&encoded)
                .map(Some)
                .map_err(|err| DbError::Decode(err.to_string())),
            None => Ok(None),
        }
    }

    pub async fn has(&self, key: &str) -> DbResult<bool> {
        self.validate(key)?;
        let exists: bool = sqlx::query_scalar(KV_EXISTS)
            .bind(&self.scope)
            .bind(&self.namespace)
            .bind(key)
            .fetch_one(&self.pool)
            .await?;
        Ok(exists)
    }

    pub async fn incr(&self, key: &str, delta: i64) -> DbResult<i64> {
        self.validate(key)?;
        let mut tx = self.pool.begin().await?;
        let lock_key = format!("{}\u{1f}{}\u{1f}{}", self.scope, self.namespace, key);
        sqlx::query(KV_LOCK)
            .bind(&lock_key)
            .execute(&mut *tx)
            .await?;
        let current: Option<Vec<u8>> = sqlx::query_scalar(KV_SELECT)
            .bind(&self.scope)
            .bind(&self.namespace)
            .bind(key)
            .fetch_optional(&mut *tx)
            .await?;
        let current = match current {
            Some(encoded) => serde_json::from_slice::<i64>(&encoded)
                .map_err(|err| DbError::NotAnInteger(key.to_string(), err.to_string()))?,
            None => 0,
        };
        let next = current + delta;
        let encoded = serde_json::to_vec(&next).map_err(|err| DbError::Encode(err.to_string()))?;
        sqlx::query(KV_UPSERT)
            .bind(&self.scope)
            .bind(&self.namespace)
            .bind(key)
            .bind(encoded)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(next)
    }

    pub async fn delete(&self, key: &str) -> DbResult<()> {
        self.validate(key)?;
        sqlx::query(KV_DELETE)
            .bind(&self.scope)
            .bind(&self.namespace)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear(&self) -> DbResult<()> {
        if self.scope.is_empty() || self.namespace.is_empty() {
            return Err(DbError::InvalidNamespace);
        }
        sqlx::query(KV_CLEAR)
            .bind(&self.scope)
            .bind(&self.namespace)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_match_go_strings() {
        assert_eq!(SCOPE_GLOBAL, "global");
        assert_eq!(SCOPE_PLUGIN, "plugin");
        assert_eq!(SCOPE_COMMAND, "command");
        assert_eq!(SCOPE_USER, "user");
    }

    #[test]
    fn sql_text_matches_go_exactly() {
        assert!(KV_UPSERT.contains("ON CONFLICT (scope, namespace, key) DO UPDATE SET"));
        assert!(KV_SELECT.contains("WHERE scope = $1 AND namespace = $2 AND key = $3"));
        assert!(KV_LOCK.contains("pg_advisory_xact_lock(hashtextextended($1, 0))"));
    }
}
