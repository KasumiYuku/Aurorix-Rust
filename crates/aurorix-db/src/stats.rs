use std::collections::BTreeMap;

use aurorix_core::Scene;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::DbResult;

pub const KEEP_DAYS: i64 = 90;

pub const TOP_LIMIT: i64 = 10;

pub const CONV_UPSERT: &str = "INSERT INTO stats_conversations (scene, id, name, msg_count, friend) VALUES ($1, $2, $3, $4, $5)\n\t\tON CONFLICT (scene, id) DO UPDATE SET\n\t\t\tmsg_count = stats_conversations.msg_count + excluded.msg_count,\n\t\t\tname = CASE WHEN excluded.name <> '' THEN excluded.name ELSE stats_conversations.name END,\n\t\t\tfriend = stats_conversations.friend OR excluded.friend";

pub const DAY_UPSERT: &str = "INSERT INTO stats_daily (day, recv, sent, button) VALUES ($1, $2, $3, $4)\n\t\tON CONFLICT (day) DO UPDATE SET\n\t\t\trecv = stats_daily.recv + excluded.recv,\n\t\t\tsent = stats_daily.sent + excluded.sent,\n\t\t\tbutton = stats_daily.button + excluded.button";

pub const BACKFILL_GROUPS: &str = "INSERT INTO stats_conversations (scene, id, name, msg_count)\n\t\tSELECT $1, group_id, name, msg_count FROM stats_groups\n\t\tON CONFLICT DO NOTHING";

pub const BACKFILL_PEERS: &str = "INSERT INTO stats_conversations (scene, id, name, msg_count, friend)\n\t\tSELECT $1, peer_id, '', msg_count, true FROM stats_peers\n\t\tON CONFLICT (scene, id) DO UPDATE SET friend = true";

pub const MARK_MIGRATED: &str =
    "INSERT INTO stats_migrations (name) VALUES ($1) ON CONFLICT DO NOTHING";

pub const MIGRATION_CHECK: &str = "SELECT EXISTS (SELECT 1 FROM stats_migrations WHERE name = $1)";

pub const READ_COUNTS: &str = "SELECT\n\t\t\tCOUNT(*) FILTER (WHERE scene = $1),\n\t\t\tCOUNT(*) FILTER (WHERE scene = $2),\n\t\t\tCOUNT(*) FILTER (WHERE scene = $3 OR scene = $4),\n\t\t\tCOUNT(*) FILTER (WHERE scene = $2 AND friend)\n\t\tFROM stats_conversations";

pub const TOP_CONVERSATIONS: &str = "SELECT scene, id, name, msg_count FROM stats_conversations\n\t\tORDER BY msg_count DESC, scene, id LIMIT $1";

pub const FRIEND_INSERT: &str =
    "INSERT INTO stats_conversations (scene, id, friend) VALUES ($1, $2, true)\n\t\tON CONFLICT (scene, id) DO UPDATE SET friend = true";

pub const UNFRIEND_UPDATE: &str =
    "UPDATE stats_conversations SET friend = false WHERE scene = $1 AND id = $2";

pub const TRIM_DAILY: &str = "DELETE FROM stats_daily WHERE day < $1";

pub const SUM_DAILY: &str = "SELECT COALESCE(SUM(recv),0)::bigint, COALESCE(SUM(sent),0)::bigint, COALESCE(SUM(button),0)::bigint FROM stats_daily";

pub const TODAY_DAILY: &str = "SELECT recv, sent, button FROM stats_daily WHERE day = $1";

pub const DAY_LAYOUT: &str = "%Y-%m-%d";

pub fn local_day_key(moment: chrono::DateTime<chrono::Local>) -> String {
    moment.format(DAY_LAYOUT).to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConvDelta {
    pub name: String,
    pub msg: i64,
    pub friend: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayDelta {
    pub recv: i64,
    pub sent: i64,
    pub button: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatsBatch {
    pub conversations: BTreeMap<(Scene, String), ConvDelta>,
    pub days: BTreeMap<String, DayDelta>,
    pub trim: bool,
}

impl StatsBatch {
    pub fn is_empty(&self) -> bool {
        self.conversations.is_empty() && self.days.is_empty() && !self.trim
    }

    pub fn received(&mut self, scene: Scene, id: &str, name: &str) -> &mut Self {
        let entry = self
            .conversations
            .entry((scene, id.to_string()))
            .or_default();
        entry.msg += 1;
        if entry.name.is_empty() {
            entry.name = name.to_string();
        }
        if scene == Scene::C2c {
            entry.friend = true;
        }
        self
    }

    pub fn count_day(&mut self, day: &str, recv: i64, sent: i64, button: i64) -> &mut Self {
        let entry = self.days.entry(day.to_string()).or_default();
        entry.recv += recv;
        entry.sent += sent;
        entry.button += button;
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayStat {
    pub recv: u64,
    pub sent: u64,
    pub button: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationRow {
    pub scene: Scene,
    pub id: String,
    pub name: String,
    pub msg: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub groups: i64,
    pub peers: i64,
    pub friends: i64,
    pub channels: i64,
    pub top: Vec<ConversationRow>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LifeTotals {
    pub recv: u64,
    pub sent: u64,
    pub button: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsView {
    pub groups: i64,
    pub peers: i64,
    pub friends: i64,
    pub channels: i64,
    pub total_recv: u64,
    pub total_sent: u64,
    pub today: DayStat,
    pub top_conversations: Vec<ConversationRow>,
}

#[derive(Debug, Clone)]
pub struct StatsStore {
    pool: PgPool,
    keep_days: i64,
}

impl StatsStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            keep_days: KEEP_DAYS,
        }
    }

    pub fn with_keep_days(pool: PgPool, keep_days: i64) -> Self {
        Self { pool, keep_days }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn write_batch(&self, batch: &StatsBatch) -> DbResult<()> {
        let mut tx = self.pool.begin().await?;
        for ((scene, id), delta) in &batch.conversations {
            sqlx::query(CONV_UPSERT)
                .bind(scene.as_str())
                .bind(id)
                .bind(&delta.name)
                .bind(delta.msg)
                .bind(delta.friend)
                .execute(&mut *tx)
                .await?;
        }
        for (day, delta) in &batch.days {
            sqlx::query(DAY_UPSERT)
                .bind(day)
                .bind(delta.recv)
                .bind(delta.sent)
                .bind(delta.button)
                .execute(&mut *tx)
                .await?;
        }
        if batch.trim {
            let cutoff =
                local_day_key(chrono::Local::now() - chrono::Duration::days(self.keep_days));
            sqlx::query(TRIM_DAILY)
                .bind(cutoff)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn read_counts(&self) -> DbResult<Counts> {
        let (groups, peers, channels, friends): (i64, i64, i64, i64) = sqlx::query_as(READ_COUNTS)
            .bind(Scene::Group.as_str())
            .bind(Scene::C2c.as_str())
            .bind(Scene::Channel.as_str())
            .bind(Scene::Dm.as_str())
            .fetch_one(&self.pool)
            .await?;
        let rows: Vec<(String, String, String, i64)> = sqlx::query_as(TOP_CONVERSATIONS)
            .bind(TOP_LIMIT)
            .fetch_all(&self.pool)
            .await?;
        let top = rows
            .into_iter()
            .map(|(scene, id, name, msg)| ConversationRow {
                scene: scene.parse().unwrap_or(Scene::Group),
                id,
                name,
                msg: msg.max(0) as u64,
            })
            .collect();
        Ok(Counts {
            groups,
            peers,
            friends,
            channels,
            top,
        })
    }

    pub async fn set_name(&self, scene: Scene, id: &str, name: &str) -> DbResult<()> {
        if id.is_empty() || name.trim().is_empty() {
            return Ok(());
        }
        sqlx::query(CONV_UPSERT)
            .bind(scene.as_str())
            .bind(id)
            .bind(name)
            .bind(0_i64)
            .bind(false)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn full_view(&self) -> DbResult<StatsView> {
        let (totals, _day_key, today) = self.seed_totals().await?;
        self.snapshot(totals, today).await
    }

    pub async fn snapshot(&self, totals: LifeTotals, today: DayStat) -> DbResult<StatsView> {
        let counts = self.read_counts().await?;
        Ok(StatsView {
            groups: counts.groups,
            peers: counts.peers,
            friends: counts.friends,
            channels: counts.channels,
            total_recv: totals.recv,
            total_sent: totals.sent,
            today,
            top_conversations: counts.top,
        })
    }

    pub async fn friend(&self, user_id: &str) -> DbResult<()> {
        if user_id.is_empty() {
            return Ok(());
        }
        sqlx::query(FRIEND_INSERT)
            .bind(Scene::C2c.as_str())
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn unfriend(&self, user_id: &str) -> DbResult<()> {
        if user_id.is_empty() {
            return Ok(());
        }
        sqlx::query(UNFRIEND_UPDATE)
            .bind(Scene::C2c.as_str())
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn backfill_legacy(&self) -> DbResult<()> {
        let steps: [(&str, &str, &str, Scene); 2] = [
            (
                "stats_groups",
                "stats_groups",
                BACKFILL_GROUPS,
                Scene::Group,
            ),
            ("stats_peers", "stats_peers", BACKFILL_PEERS, Scene::C2c),
        ];
        for (name, table, query, scene) in steps {
            let migrated: bool = sqlx::query_scalar(MIGRATION_CHECK)
                .bind(name)
                .fetch_one(&self.pool)
                .await?;
            if migrated {
                continue;
            }
            let present: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
                .bind(table)
                .fetch_one(&self.pool)
                .await?;
            let mut tx = self.pool.begin().await?;
            if present {
                sqlx::query(query)
                    .bind(scene.as_str())
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query(MARK_MIGRATED)
                .bind(name)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        Ok(())
    }

    pub async fn seed_totals(&self) -> DbResult<(LifeTotals, String, DayStat)> {
        let (recv, sent, button): (i64, i64, i64) =
            sqlx::query_as(SUM_DAILY).fetch_one(&self.pool).await?;
        let key = local_day_key(chrono::Local::now());
        let today: Option<(i64, i64, i64)> = sqlx::query_as(TODAY_DAILY)
            .bind(&key)
            .fetch_optional(&self.pool)
            .await?;
        let day = match today {
            Some((recv, sent, button)) => DayStat {
                recv: recv.max(0) as u64,
                sent: sent.max(0) as u64,
                button: button.max(0) as u64,
            },
            None => DayStat::default(),
        };
        Ok((
            LifeTotals {
                recv: recv.max(0) as u64,
                sent: sent.max(0) as u64,
                button: button.max(0) as u64,
            },
            key,
            day,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_accumulates_per_conversation_and_day() {
        let mut batch = StatsBatch::default();
        batch.received(Scene::Group, "g1", "群一");
        batch.received(Scene::Group, "g1", "群二");
        batch.count_day("2024-01-01", 1, 2, 3);
        batch.count_day("2024-01-01", 1, 0, 1);
        assert_eq!(
            batch.conversations[&(Scene::Group, "g1".to_string())].msg,
            2
        );
        assert_eq!(
            batch.conversations[&(Scene::Group, "g1".to_string())].name,
            "群一"
        );
        assert_eq!(batch.days["2024-01-01"].recv, 2);
        assert_eq!(batch.days["2024-01-01"].button, 4);
        assert!(!batch.is_empty());
    }

    #[test]
    fn private_messages_mark_friend() {
        let mut batch = StatsBatch::default();
        batch.received(Scene::C2c, "u1", "");
        assert!(batch.conversations[&(Scene::C2c, "u1".to_string())].friend);
    }

    #[test]
    fn upsert_sql_matches_go_semantics() {
        assert!(
            CONV_UPSERT.contains("msg_count = stats_conversations.msg_count + excluded.msg_count")
        );
        assert!(CONV_UPSERT.contains("friend = stats_conversations.friend OR excluded.friend"));
        assert!(DAY_UPSERT.contains("recv = stats_daily.recv + excluded.recv"));
        assert!(READ_COUNTS.contains("COUNT(*) FILTER (WHERE scene = $2 AND friend)"));
    }

    #[test]
    fn day_key_uses_local_timezone_not_utc() {
        use chrono::TimeZone;
        let midnight = chrono::Local
            .with_ymd_and_hms(2024, 3, 1, 0, 30, 0)
            .single()
            .expect("本地时间");
        assert_eq!(local_day_key(midnight), "2024-03-01");
        let late = chrono::Local
            .with_ymd_and_hms(2024, 3, 1, 23, 59, 59)
            .single()
            .expect("本地时间");
        assert_eq!(local_day_key(late), "2024-03-01");
        assert_ne!(
            local_day_key(midnight),
            local_day_key(midnight + chrono::Duration::days(1))
        );
    }

    #[test]
    fn keep_days_and_top_limit_match_go() {
        assert_eq!(KEEP_DAYS, 90);
        assert_eq!(TOP_LIMIT, 10);
    }

    #[test]
    fn batch_is_empty_only_when_every_part_is_empty() {
        let mut only_day = StatsBatch::default();
        only_day.count_day("2024-01-02", 1, 0, 0);
        assert!(!only_day.is_empty(), "只有日统计的批次不算空");

        let mut only_conversation = StatsBatch::default();
        only_conversation.received(Scene::Group, "g2", "群");
        assert!(!only_conversation.is_empty(), "只有会话统计的批次不算空");
        let trim_only = StatsBatch {
            trim: true,
            ..StatsBatch::default()
        };
        assert!(!trim_only.is_empty(), "只要求裁剪的批次不算空");

        assert!(StatsBatch::default().is_empty(), "全新批次应当是空的");
    }

    #[test]
    fn backfill_peers_marks_friend_true() {
        assert!(
            BACKFILL_PEERS.contains("msg_count, true FROM stats_peers"),
            "回填私聊时应当把 friend 标成 true"
        );
    }
}
