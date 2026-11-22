use aurorix_core::Scene;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::error::DbResult;

pub const MAX_PER_CONVERSATION: i64 = 500;

pub const MAX_AGE_DAYS: i64 = 90;

pub const PREVIEW_RUNES: usize = 60;

pub const DEFAULT_PAGE_SIZE: i64 = 50;

pub const MAX_PAGE_SIZE: i64 = 200;
pub type MessageRow = (i64, String, String, String, String, String, String, i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Direction::Inbound => "in",
            Direction::Outbound => "out",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "out" => Direction::Outbound,
            _ => Direction::Inbound,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub kind: Scene,
    pub name: String,
    pub last_at: Option<DateTime<Utc>>,
    pub last_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: i64,
    pub conv_id: String,
    pub kind: Scene,
    pub direction: Direction,
    pub sender_id: String,
    pub sender_name: String,
    pub content: String,
    pub platform_id: String,
    pub created_at: DateTime<Utc>,
}

pub const INSERT_MESSAGE: &str = "INSERT INTO inbox_messages (conv_id, direction, sender_id, sender_name, content, platform_msg_id, created_at)\n\t\t VALUES ($1, $2, $3, $4, $5, $6, $7)";

pub const UPSERT_CONVERSATION: &str = "INSERT INTO inbox_conversations (id, kind, name, last_at, last_preview) VALUES ($1, $2, '', $3, $4)\n\t\t ON CONFLICT (id) DO UPDATE SET\n\t\t   kind = excluded.kind,\n\t\t   last_at = GREATEST(inbox_conversations.last_at, excluded.last_at),\n\t\t   last_preview = CASE\n\t\t     WHEN excluded.last_at >= inbox_conversations.last_at THEN excluded.last_preview\n\t\t     ELSE inbox_conversations.last_preview END";

pub const SELECT_CONVERSATIONS: &str =
    "SELECT id, kind, name, last_at, last_preview FROM inbox_conversations ORDER BY last_at DESC, id";

pub const SELECT_CONVERSATIONS_PAGE: &str = "SELECT id, kind, name, last_at, last_preview FROM inbox_conversations ORDER BY last_at DESC, id LIMIT $1 OFFSET $2";

pub const SELECT_CONVERSATION: &str =
    "SELECT id, kind, name, last_at, last_preview FROM inbox_conversations WHERE id = $1";

pub const UPDATE_NAME: &str =
    "UPDATE inbox_conversations SET name = $1 WHERE id = $2 AND name IS DISTINCT FROM $1";

pub const SELECT_NAME: &str = "SELECT name FROM inbox_conversations WHERE id = $1";

pub const TRIM_AGE: &str = "DELETE FROM inbox_messages WHERE created_at < $1";

pub const TRIM_FULL_CONVERSATIONS: &str =
    "SELECT conv_id FROM inbox_messages GROUP BY conv_id HAVING COUNT(*) > $1";

pub const TRIM_CONVERSATION: &str = "DELETE FROM inbox_messages WHERE conv_id = $1 AND id <= (\n\t\t\t   SELECT id FROM inbox_messages WHERE conv_id = $2 ORDER BY id DESC LIMIT 1 OFFSET $3)";

pub const SEED_FROM_STATS: &str = "INSERT INTO inbox_conversations (id, kind, name, last_at, last_preview)\n\t\t SELECT id, scene, name, 0, '' FROM stats_conversations\n\t\t ON CONFLICT (id) DO NOTHING";

#[derive(Debug, Clone)]
pub struct InboxStore {
    pool: PgPool,
}

impl InboxStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn save(&self, message: &Message) -> DbResult<()> {
        let at = message.created_at.timestamp();
        let mut tx = self.pool.begin().await?;
        sqlx::query(INSERT_MESSAGE)
            .bind(&message.conv_id)
            .bind(message.direction.as_str())
            .bind(&message.sender_id)
            .bind(&message.sender_name)
            .bind(&message.content)
            .bind(&message.platform_id)
            .bind(at)
            .execute(&mut *tx)
            .await?;
        sqlx::query(UPSERT_CONVERSATION)
            .bind(&message.conv_id)
            .bind(message.kind.as_str())
            .bind(at)
            .bind(preview(&message.content))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn conversations(&self) -> DbResult<Vec<Conversation>> {
        let rows: Vec<(String, String, String, i64, String)> = sqlx::query_as(SELECT_CONVERSATIONS)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|(id, kind, name, at, last_preview)| Conversation {
                id,
                kind: kind.parse().unwrap_or(Scene::Group),
                name,
                last_at: unix_time(at),
                last_preview,
            })
            .collect())
    }

    pub async fn conversations_page(&self, limit: i64, offset: i64) -> DbResult<Vec<Conversation>> {
        let limit = if limit <= 0 || limit > MAX_PAGE_SIZE {
            DEFAULT_PAGE_SIZE
        } else {
            limit
        };
        let offset = if offset < 0 { 0 } else { offset };
        let rows: Vec<(String, String, String, i64, String)> =
            sqlx::query_as(SELECT_CONVERSATIONS_PAGE)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(id, kind, name, at, last_preview)| Conversation {
                id,
                kind: kind.parse().unwrap_or(Scene::Group),
                name,
                last_at: unix_time(at),
                last_preview,
            })
            .collect())
    }

    pub async fn lookup(&self, conv_id: &str) -> DbResult<Option<Conversation>> {
        if conv_id.is_empty() {
            return Ok(None);
        }
        let row: Option<(String, String, String, i64, String)> =
            sqlx::query_as(SELECT_CONVERSATION)
                .bind(conv_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(id, kind, name, at, last_preview)| Conversation {
            id,
            kind: kind.parse().unwrap_or(Scene::Group),
            name,
            last_at: unix_time(at),
            last_preview,
        }))
    }

    pub async fn messages(&self, conv_id: &str, before: i64, limit: i64) -> DbResult<Vec<Message>> {
        if conv_id.is_empty() {
            return Ok(Vec::new());
        }
        let limit = if limit <= 0 || limit > MAX_PAGE_SIZE {
            DEFAULT_PAGE_SIZE
        } else {
            limit
        };
        let rows: Vec<MessageRow> = if before > 0 {
            sqlx::query_as(
                "SELECT id, conv_id, direction, sender_id, sender_name, content, platform_msg_id, created_at\n\t          FROM inbox_messages WHERE conv_id = $1 AND id < $2 ORDER BY id DESC LIMIT $3",
            )
            .bind(conv_id)
            .bind(before)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as(
                "SELECT id, conv_id, direction, sender_id, sender_name, content, platform_msg_id, created_at\n\t          FROM inbox_messages WHERE conv_id = $1 ORDER BY id DESC LIMIT $2",
            )
            .bind(conv_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };
        let kind = self
            .lookup(conv_id)
            .await
            .ok()
            .flatten()
            .map(|conv| conv.kind)
            .unwrap_or(Scene::Group);
        let mut messages: Vec<Message> = rows
            .into_iter()
            .map(
                |(id, conv_id, direction, sender_id, sender_name, content, platform_id, at)| {
                    Message {
                        id,
                        conv_id,
                        kind,
                        direction: Direction::parse(&direction),
                        sender_id,
                        sender_name,
                        content,
                        platform_id,
                        created_at: DateTime::<Utc>::from_timestamp(at, 0).unwrap_or_default(),
                    }
                },
            )
            .collect();
        messages.reverse();
        Ok(messages)
    }

    pub async fn set_name(&self, conv_id: &str, name: &str) -> DbResult<()> {
        if conv_id.is_empty() || name.trim().is_empty() {
            return Ok(());
        }
        sqlx::query(UPDATE_NAME)
            .bind(name)
            .bind(conv_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn name_of(&self, conv_id: &str) -> DbResult<String> {
        if conv_id.is_empty() {
            return Ok(String::new());
        }
        let name: Option<String> = sqlx::query_scalar(SELECT_NAME)
            .bind(conv_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(name.unwrap_or_default())
    }

    pub async fn trim(&self) -> DbResult<()> {
        let cutoff = Utc::now().timestamp() - MAX_AGE_DAYS * 24 * 60 * 60;
        sqlx::query(TRIM_AGE)
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        let full: Vec<String> = sqlx::query_scalar(TRIM_FULL_CONVERSATIONS)
            .bind(MAX_PER_CONVERSATION)
            .fetch_all(&self.pool)
            .await?;
        for conv in full {
            sqlx::query(TRIM_CONVERSATION)
                .bind(&conv)
                .bind(&conv)
                .bind(MAX_PER_CONVERSATION)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn seed(&self) -> DbResult<()> {
        sqlx::query(SEED_FROM_STATS).execute(&self.pool).await?;
        Ok(())
    }
}

fn unix_time(seconds: i64) -> Option<DateTime<Utc>> {
    if seconds <= 0 {
        return None;
    }
    DateTime::<Utc>::from_timestamp(seconds, 0)
}

pub fn preview(content: &str) -> String {
    let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");
    let runes: Vec<char> = collapsed.chars().collect();
    if runes.len() > PREVIEW_RUNES {
        let mut out: String = runes[..PREVIEW_RUNES].iter().collect();
        out.push('…');
        return out;
    }
    collapsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_collapses_whitespace() {
        assert_eq!(preview("a \n\t b"), "a b");
    }

    #[test]
    fn preview_truncates_at_sixty_runes() {
        let long = "字".repeat(PREVIEW_RUNES + 5);
        let cut = preview(&long);
        assert_eq!(cut.chars().count(), PREVIEW_RUNES + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn direction_round_trips() {
        assert_eq!(Direction::Inbound.as_str(), "in");
        assert_eq!(Direction::Outbound.as_str(), "out");
        assert_eq!(Direction::parse("out"), Direction::Outbound);
        assert_eq!(Direction::parse("in"), Direction::Inbound);
        assert_eq!(Direction::parse("bogus"), Direction::Inbound);
    }

    #[test]
    fn page_limits_match_go() {
        assert_eq!(DEFAULT_PAGE_SIZE, 50);
        assert_eq!(MAX_PAGE_SIZE, 200);
        assert_eq!(MAX_PER_CONVERSATION, 500);
        assert_eq!(MAX_AGE_DAYS, 90);
    }

    #[test]
    fn schema_sql_keeps_identity_and_bigint_timestamps() {
        assert!(crate::schema::INBOX_MESSAGES.contains("GENERATED ALWAYS AS IDENTITY PRIMARY KEY"));
        assert!(crate::schema::INBOX_MESSAGES.contains("created_at      BIGINT NOT NULL"));
        assert!(
            UPSERT_CONVERSATION.contains("GREATEST(inbox_conversations.last_at, excluded.last_at)")
        );
    }

    #[test]
    fn unix_time_rejects_non_positive_seconds() {
        assert!(unix_time(0).is_none(), "0 秒应当视为没有时间");
        assert!(unix_time(-1).is_none(), "负数秒应当视为没有时间");
        let at = unix_time(1_700_000_000).expect("正数秒应当可解析");
        assert_eq!(at.timestamp(), 1_700_000_000);
    }

    #[test]
    fn preview_keeps_exactly_sixty_runes() {
        let exact = "字".repeat(PREVIEW_RUNES);
        let kept = preview(&exact);
        assert_eq!(kept, exact, "正好 60 个字符时不该截断");
        assert!(!kept.ends_with('…'), "正好 60 个字符时不该加省略号");
        let over = "字".repeat(PREVIEW_RUNES + 1);
        let cut = preview(&over);
        assert_eq!(
            cut.chars().count(),
            PREVIEW_RUNES + 1,
            "61 个字符应当保留 60 加省略号"
        );
        assert!(cut.ends_with('…'), "61 个字符时应当加省略号");
    }

    #[test]
    fn sql_text_keeps_latest_wins_and_trim_guards() {
        assert!(
            UPSERT_CONVERSATION.contains("WHEN excluded.last_at >= inbox_conversations.last_at"),
            "会话预览只在时间不落后时才覆盖"
        );
        assert!(
            TRIM_CONVERSATION.contains("id <= ("),
            "裁剪应当保留分界消息本身"
        );
    }
}
