use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use chrono::{Local, SecondsFormat};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{DbError, DbResult};
use crate::kv::KvStore;

pub type AdminFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub const DEFAULT_BAN_MESSAGE: &str = "您的账号因违反Bot使用协议已被封禁";

pub const BAN_MESSAGE_LIMIT: usize = 2000;

pub const ROSTER_KEY: &str = "roster";

pub const PANEL_KEY: &str = "menu:panel";

pub const SNAPSHOT_INDEX_KEY: &str = "menu:snapshot:index";

pub const SNAPSHOT_KEY_PREFIX: &str = "menu:snapshot:";

pub const PANEL_SNAPSHOT_LIMIT: usize = 20;

pub const PANEL_TARGET_ALL: &str = "all";

pub const ASSET_PROVIDERS_KEY: &str = "assets:providers";

pub const ASSET_WHITELIST_KEY: &str = "assets:whitelist";

pub trait AdminKv: Send + Sync {
    fn get<'a>(&'a self, key: &'a str) -> AdminFuture<'a, DbResult<Option<Value>>>;

    fn set<'a>(&'a self, key: &'a str, value: Value) -> AdminFuture<'a, DbResult<()>>;

    fn remove<'a>(&'a self, key: &'a str) -> AdminFuture<'a, DbResult<()>>;
}

#[derive(Debug, Clone)]
pub struct PgKv {
    kv: KvStore,
}

impl PgKv {
    pub fn new(kv: KvStore) -> Self {
        Self { kv }
    }

    pub fn kv(&self) -> &KvStore {
        &self.kv
    }
}

impl AdminKv for PgKv {
    fn get<'a>(&'a self, key: &'a str) -> AdminFuture<'a, DbResult<Option<Value>>> {
        Box::pin(async move { self.kv.get::<Value>(key).await })
    }

    fn set<'a>(&'a self, key: &'a str, value: Value) -> AdminFuture<'a, DbResult<()>> {
        Box::pin(async move { self.kv.set(key, &value).await })
    }

    fn remove<'a>(&'a self, key: &'a str) -> AdminFuture<'a, DbResult<()>> {
        Box::pin(async move { self.kv.delete(key).await })
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

pub fn now_rfc3339() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

fn encode<T: Serialize + ?Sized>(value: &T) -> DbResult<Value> {
    serde_json::to_value(value).map_err(|err| DbError::Encode(err.to_string()))
}

fn decode<T: DeserializeOwned>(value: Option<Value>, what: &str) -> DbResult<Option<T>> {
    match value {
        Some(raw) => serde_json::from_value(raw)
            .map(Some)
            .map_err(|err| DbError::Decode(format!("{what}: {err}"))),
        None => Ok(None),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BanEntry {
    #[serde(default)]
    pub openid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub at: String,
}

impl BanEntry {
    pub fn new(openid: impl Into<String>) -> Self {
        Self {
            openid: openid.into(),
            ..Self::default()
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    pub fn with_at(mut self, at: impl Into<String>) -> Self {
        self.at = at.into();
        self
    }

    pub fn text(&self) -> &str {
        if self.message.trim().is_empty() {
            return DEFAULT_BAN_MESSAGE;
        }
        &self.message
    }

    pub fn validate(&self) -> DbResult<()> {
        if self.openid.trim().is_empty() {
            return Err(DbError::Invalid("openid 不能为空".to_string()));
        }
        if self
            .openid
            .chars()
            .any(|ch| ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n')
        {
            return Err(DbError::Invalid("openid 不能包含空白字符".to_string()));
        }
        let length = self.message.chars().count();
        if length > BAN_MESSAGE_LIMIT {
            return Err(DbError::Invalid(format!(
                "文案 {length} 字, 超过上限 {BAN_MESSAGE_LIMIT} 字"
            )));
        }
        Ok(())
    }
}

fn by_openid(mut entries: Vec<BanEntry>) -> Vec<BanEntry> {
    entries.sort_by(|left, right| left.openid.cmp(&right.openid));
    entries
}

fn by_recent(mut entries: Vec<BanEntry>) -> Vec<BanEntry> {
    entries.sort_by(|left, right| {
        right
            .at
            .cmp(&left.at)
            .then_with(|| left.openid.cmp(&right.openid))
    });
    entries
}

#[derive(Clone)]
pub struct BanStore {
    kv: Arc<dyn AdminKv>,
}

impl std::fmt::Debug for BanStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("BanStore")
    }
}

impl BanStore {
    pub fn new(kv: Arc<dyn AdminKv>) -> Self {
        Self { kv }
    }

    pub fn from_kv(kv: KvStore) -> Self {
        Self::new(Arc::new(PgKv::new(kv)))
    }

    pub async fn load(&self) -> DbResult<Vec<BanEntry>> {
        let encoded = self.kv.get(ROSTER_KEY).await?;
        let entries = decode::<Vec<BanEntry>>(encoded, ROSTER_KEY)?.unwrap_or_default();
        Ok(by_recent(entries))
    }

    pub async fn save(&self, entries: &[BanEntry]) -> DbResult<()> {
        for entry in entries {
            entry.validate()?;
        }
        let value = encode(&by_openid(entries.to_vec()))?;
        self.kv.set(ROSTER_KEY, value).await
    }

    pub async fn add(&self, entry: BanEntry) -> DbResult<BanEntry> {
        entry.validate()?;
        let mut entries = self.load().await?;
        let mut entry = entry;
        if let Some(previous) = entries.iter().find(|item| item.openid == entry.openid) {
            if entry.at.is_empty() {
                entry.at = previous.at.clone();
            }
        }
        if entry.at.is_empty() {
            entry.at = now_rfc3339();
        }
        entries.retain(|item| item.openid != entry.openid);
        entries.push(entry.clone());
        self.save(&entries).await?;
        Ok(entry)
    }

    pub async fn remove(&self, openid: &str) -> DbResult<()> {
        let openid = openid.trim();
        let mut entries = self.load().await?;
        let before = entries.len();
        entries.retain(|item| item.openid != openid);
        if entries.len() == before {
            return Err(DbError::Invalid("该用户不在封禁名单里".to_string()));
        }
        self.save(&entries).await
    }

    pub async fn clear(&self) -> DbResult<()> {
        self.save(&[]).await
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelItem {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub only_admin: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub link: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelBody {
    #[serde(default)]
    pub items: Vec<PanelItem>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remark: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub version: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelRecord {
    #[serde(skip)]
    pub panel_id: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub target_type: String,
    #[serde(default)]
    pub panel: PanelBody,
    #[serde(default)]
    pub version: i32,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotIndexEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub panel_id: String,
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub saved_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelSnapshot {
    pub name: String,
    pub panel_id: String,
    pub seq: u64,
    pub action: String,
    pub saved_at: String,
    pub record: PanelRecord,
}

pub fn snapshot_key(panel_id: &str, seq: u64) -> String {
    format!("{SNAPSHOT_KEY_PREFIX}{panel_id}:{seq}")
}

pub fn snapshot_name(panel_id: &str, seq: u64) -> String {
    format!("{panel_id}-{seq:06}.json")
}

fn next_panel_id(map: &BTreeMap<String, PanelRecord>) -> String {
    let mut seq = map.len() + 1;
    loop {
        let candidate = format!("p_{seq}");
        if !map.contains_key(&candidate) {
            return candidate;
        }
        seq += 1;
    }
}

#[derive(Clone)]
pub struct PanelStore {
    kv: Arc<dyn AdminKv>,
}

impl std::fmt::Debug for PanelStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PanelStore")
    }
}

impl PanelStore {
    pub fn new(kv: Arc<dyn AdminKv>) -> Self {
        Self { kv }
    }

    pub fn from_kv(kv: KvStore) -> Self {
        Self::new(Arc::new(PgKv::new(kv)))
    }

    async fn load_map(&self) -> DbResult<BTreeMap<String, PanelRecord>> {
        let encoded = self.kv.get(PANEL_KEY).await?;
        let map = decode::<BTreeMap<String, PanelRecord>>(encoded, PANEL_KEY)?.unwrap_or_default();
        let mut out = BTreeMap::new();
        for (panel_id, mut record) in map {
            record.panel_id = panel_id.clone();
            out.insert(panel_id, record);
        }
        Ok(out)
    }

    async fn store_map(&self, map: &BTreeMap<String, PanelRecord>) -> DbResult<()> {
        let value = encode(map)?;
        self.kv.set(PANEL_KEY, value).await
    }

    async fn load_index(&self) -> DbResult<Vec<SnapshotIndexEntry>> {
        let encoded = self.kv.get(SNAPSHOT_INDEX_KEY).await?;
        Ok(decode::<Vec<SnapshotIndexEntry>>(encoded, SNAPSHOT_INDEX_KEY)?.unwrap_or_default())
    }

    async fn store_index(&self, index: &[SnapshotIndexEntry]) -> DbResult<()> {
        let mut sorted = index.to_vec();
        sorted.sort_by(|left, right| {
            left.panel_id
                .cmp(&right.panel_id)
                .then_with(|| left.seq.cmp(&right.seq))
        });
        let value = encode(&sorted)?;
        self.kv.set(SNAPSHOT_INDEX_KEY, value).await
    }

    pub async fn all(&self) -> DbResult<Vec<PanelRecord>> {
        let mut records: Vec<PanelRecord> = self.load_map().await?.into_values().collect();
        records.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.panel_id.cmp(&right.panel_id))
        });
        Ok(records)
    }

    pub async fn list(&self, scope: &str) -> DbResult<Vec<PanelRecord>> {
        let records = self.all().await?;
        Ok(records
            .into_iter()
            .filter(|record| record.scope == scope)
            .collect())
    }

    pub async fn count(&self) -> DbResult<usize> {
        Ok(self.load_map().await?.len())
    }

    pub async fn get(&self, panel_id: &str) -> DbResult<Option<PanelRecord>> {
        let map = self.load_map().await?;
        Ok(map.get(panel_id).cloned())
    }

    pub async fn create(&self, scope: &str, panel: PanelBody) -> DbResult<PanelRecord> {
        let mut map = self.load_map().await?;
        let panel_id = next_panel_id(&map);
        let record = PanelRecord {
            panel_id: panel_id.clone(),
            scope: scope.to_string(),
            target_type: PANEL_TARGET_ALL.to_string(),
            panel: PanelBody {
                version: 1,
                ..panel
            },
            version: 1,
            updated_at: now_rfc3339(),
        };
        map.insert(panel_id, record.clone());
        self.store_map(&map).await?;
        Ok(record)
    }

    pub async fn upsert(&self, record: PanelRecord) -> DbResult<PanelRecord> {
        let mut map = self.load_map().await?;
        let version = match map.get(&record.panel_id) {
            Some(previous) => previous.version + 1,
            None => 1,
        };
        let mut next = record;
        next.version = version;
        next.panel.version = version;
        if next.target_type.is_empty() {
            next.target_type = PANEL_TARGET_ALL.to_string();
        }
        next.updated_at = now_rfc3339();
        map.insert(next.panel_id.clone(), next.clone());
        self.store_map(&map).await?;
        Ok(next)
    }

    pub async fn remove(&self, panel_id: &str) -> DbResult<bool> {
        let mut map = self.load_map().await?;
        if map.remove(panel_id).is_none() {
            return Ok(false);
        }
        self.store_map(&map).await?;
        Ok(true)
    }

    pub async fn snapshots(&self, panel_id: &str) -> DbResult<Vec<PanelSnapshot>> {
        let index = self.load_index().await?;
        let mut entries: Vec<SnapshotIndexEntry> = index
            .into_iter()
            .filter(|entry| entry.panel_id == panel_id)
            .collect();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.seq));
        self.materialize(entries).await
    }

    pub async fn all_snapshots(&self) -> DbResult<Vec<PanelSnapshot>> {
        let index = self.load_index().await?;
        let mut entries = index;
        entries.sort_by(|left, right| {
            right
                .saved_at
                .cmp(&left.saved_at)
                .then_with(|| right.name.cmp(&left.name))
        });
        self.materialize(entries).await
    }

    async fn materialize(&self, entries: Vec<SnapshotIndexEntry>) -> DbResult<Vec<PanelSnapshot>> {
        let mut out = Vec::with_capacity(entries.len());
        for entry in entries {
            let key = snapshot_key(&entry.panel_id, entry.seq);
            let encoded = self.kv.get(&key).await?;
            let Some(mut record) = decode::<PanelRecord>(encoded, &key)? else {
                continue;
            };
            record.panel_id = entry.panel_id.clone();
            out.push(PanelSnapshot {
                name: entry.name,
                panel_id: entry.panel_id,
                seq: entry.seq,
                action: entry.action,
                saved_at: entry.saved_at,
                record,
            });
        }
        Ok(out)
    }

    pub async fn snapshot(&self, name: &str) -> DbResult<Option<PanelSnapshot>> {
        let index = self.load_index().await?;
        let Some(entry) = index.into_iter().find(|entry| entry.name == name) else {
            return Ok(None);
        };
        let key = snapshot_key(&entry.panel_id, entry.seq);
        let encoded = self.kv.get(&key).await?;
        let Some(mut record) = decode::<PanelRecord>(encoded, &key)? else {
            return Ok(None);
        };
        record.panel_id = entry.panel_id.clone();
        Ok(Some(PanelSnapshot {
            name: entry.name,
            panel_id: entry.panel_id,
            seq: entry.seq,
            action: entry.action,
            saved_at: entry.saved_at,
            record,
        }))
    }

    pub async fn push_snapshot(
        &self,
        record: &PanelRecord,
        action: &str,
    ) -> DbResult<PanelSnapshot> {
        let mut index = self.load_index().await?;
        let seq = index
            .iter()
            .filter(|entry| entry.panel_id == record.panel_id)
            .map(|entry| entry.seq)
            .max()
            .unwrap_or(0)
            + 1;
        let name = snapshot_name(&record.panel_id, seq);
        let saved_at = now_rfc3339();
        let stored = record.clone();
        let key = snapshot_key(&record.panel_id, seq);
        self.kv.set(&key, encode(&stored)?).await?;
        index.push(SnapshotIndexEntry {
            name: name.clone(),
            panel_id: record.panel_id.clone(),
            seq,
            action: action.to_string(),
            saved_at: saved_at.clone(),
        });
        let mut own: Vec<SnapshotIndexEntry> = index
            .iter()
            .filter(|entry| entry.panel_id == record.panel_id)
            .cloned()
            .collect();
        own.sort_by_key(|entry| entry.seq);
        while own.len() > PANEL_SNAPSHOT_LIMIT {
            let dropped = own.remove(0);
            index.retain(|entry| entry.name != dropped.name);
            self.kv
                .remove(&snapshot_key(&dropped.panel_id, dropped.seq))
                .await?;
        }
        self.store_index(&index).await?;
        Ok(PanelSnapshot {
            name,
            panel_id: record.panel_id.clone(),
            seq,
            action: action.to_string(),
            saved_at,
            record: stored,
        })
    }

    pub async fn restore(&self, panel_id: &str, seq: u64) -> DbResult<PanelRecord> {
        let key = snapshot_key(panel_id, seq);
        let encoded = self.kv.get(&key).await?;
        let Some(mut record) = decode::<PanelRecord>(encoded, &key)? else {
            return Err(DbError::Invalid("快照不存在".to_string()));
        };
        record.panel_id = panel_id.to_string();
        self.upsert(record).await
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssetProvider {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "is_zero_priority")]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<bool>,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub config: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone)]
pub struct AssetStore {
    kv: Arc<dyn AdminKv>,
}

fn is_zero_priority(value: &i32) -> bool {
    *value == 0
}

impl std::fmt::Debug for AssetStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AssetStore")
    }
}

impl AssetStore {
    pub fn new(kv: Arc<dyn AdminKv>) -> Self {
        Self { kv }
    }

    pub fn from_kv(kv: KvStore) -> Self {
        Self::new(Arc::new(PgKv::new(kv)))
    }

    pub async fn load(&self) -> DbResult<Vec<AssetProvider>> {
        let encoded = self.kv.get(ASSET_PROVIDERS_KEY).await?;
        Ok(decode::<Vec<AssetProvider>>(encoded, ASSET_PROVIDERS_KEY)?.unwrap_or_default())
    }

    pub async fn save(&self, providers: &[AssetProvider]) -> DbResult<()> {
        let value = encode(providers)?;
        self.kv.set(ASSET_PROVIDERS_KEY, value).await
    }

    pub async fn load_whitelist(&self) -> DbResult<Vec<String>> {
        let encoded = self.kv.get(ASSET_WHITELIST_KEY).await?;
        Ok(decode::<Vec<String>>(encoded, ASSET_WHITELIST_KEY)?.unwrap_or_default())
    }

    pub async fn save_whitelist(&self, whitelist: &[String]) -> DbResult<()> {
        let value = encode(whitelist)?;
        self.kv.set(ASSET_WHITELIST_KEY, value).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeKv {
        values: Mutex<BTreeMap<String, Value>>,
    }

    impl FakeKv {
        fn shared() -> Arc<Self> {
            Arc::new(Self::default())
        }

        fn raw(&self, key: &str) -> Option<Value> {
            self.values.lock().expect("锁假 KV").get(key).cloned()
        }

        fn keys(&self) -> Vec<String> {
            self.values
                .lock()
                .expect("锁假 KV")
                .keys()
                .cloned()
                .collect()
        }
    }

    impl AdminKv for FakeKv {
        fn get<'a>(&'a self, key: &'a str) -> AdminFuture<'a, DbResult<Option<Value>>> {
            Box::pin(async move {
                let value = { self.values.lock().expect("锁假 KV").get(key).cloned() };
                Ok(value)
            })
        }

        fn set<'a>(&'a self, key: &'a str, value: Value) -> AdminFuture<'a, DbResult<()>> {
            Box::pin(async move {
                self.values
                    .lock()
                    .expect("锁假 KV")
                    .insert(key.to_string(), value);
                Ok(())
            })
        }

        fn remove<'a>(&'a self, key: &'a str) -> AdminFuture<'a, DbResult<()>> {
            Box::pin(async move {
                self.values.lock().expect("锁假 KV").remove(key);
                Ok(())
            })
        }
    }

    fn item(name: &str) -> PanelItem {
        PanelItem {
            name: name.to_string(),
            desc: "查分".to_string(),
            kind: "command".to_string(),
            ..PanelItem::default()
        }
    }

    fn body(version: i32) -> PanelBody {
        PanelBody {
            items: vec![item("查询")],
            remark: "面板".to_string(),
            version,
        }
    }

    #[tokio::test]
    async fn roster_key_and_json_shape_match_go() {
        let kv = FakeKv::shared();
        let store = BanStore::new(kv.clone());
        store
            .add(
                BanEntry::new("user-1")
                    .with_message("别再来了")
                    .with_note("刷屏")
                    .with_at("2024-01-01T00:00:00Z"),
            )
            .await
            .expect("写入");
        store
            .add(BanEntry::new("user-2").with_at("2024-01-02T00:00:00Z"))
            .await
            .expect("写入");
        let raw = kv.raw("roster").expect("roster 键");
        let list = raw.as_array().expect("数组");
        assert_eq!(list.len(), 2);
        assert_eq!(
            kv.keys(),
            vec!["roster".to_string()],
            "封禁名单只写到 roster 键"
        );
        assert_eq!(
            list[0],
            serde_json::json!({
                "openid": "user-1",
                "message": "别再来了",
                "note": "刷屏",
                "at": "2024-01-01T00:00:00Z",
            })
        );
        assert_eq!(
            list[1],
            serde_json::json!({ "openid": "user-2", "at": "2024-01-02T00:00:00Z" }),
            "空 message/note 不落库"
        );
        let loaded = store.load().await.expect("读回");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].openid, "user-2", "列表按封禁时间倒序, 最近的在前");
        assert_eq!(loaded[0].at, "2024-01-02T00:00:00Z");
    }

    #[tokio::test]
    async fn ban_message_cap_counts_runes_and_rejects_2001() {
        let kv = FakeKv::shared();
        let store = BanStore::new(kv.clone());
        let exact = "字".repeat(BAN_MESSAGE_LIMIT);
        store
            .add(BanEntry::new("user-1").with_message(exact))
            .await
            .expect("正好 2000 字可以存");
        let over = "字".repeat(BAN_MESSAGE_LIMIT + 1);
        let err = store
            .add(BanEntry::new("user-2").with_message(over))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "文案 2001 字, 超过上限 2000 字");
        assert_eq!(BAN_MESSAGE_LIMIT, 2000);
        assert_eq!(DEFAULT_BAN_MESSAGE, "您的账号因违反Bot使用协议已被封禁");
        let loaded = store.load().await.expect("读回");
        assert_eq!(loaded.len(), 1, "超长文案不入库");
        assert_eq!(loaded[0].text(), loaded[0].message);
        assert_eq!(
            BanEntry::new("user-3").text(),
            DEFAULT_BAN_MESSAGE,
            "空文案回落到默认文案"
        );
    }

    #[tokio::test]
    async fn ban_openid_validation_matches_go_messages() {
        let kv = FakeKv::shared();
        let store = BanStore::new(kv);
        let err = store
            .add(BanEntry::new("   "))
            .await
            .expect_err("空 openid");
        assert_eq!(err.to_string(), "openid 不能为空");
        let err = store.add(BanEntry::new("a b")).await.expect_err("含空白");
        assert_eq!(err.to_string(), "openid 不能包含空白字符");
        let err = store.remove("user-404").await.expect_err("不在名单");
        assert_eq!(err.to_string(), "该用户不在封禁名单里");
    }

    #[tokio::test]
    async fn ban_at_passthrough_and_autostamp() {
        let kv = FakeKv::shared();
        let store = BanStore::new(kv.clone());
        let entry = store
            .add(BanEntry::new("user-1").with_at("2024-05-06T07:08:09+08:00"))
            .await
            .expect("写入");
        assert_eq!(entry.at, "2024-05-06T07:08:09+08:00", "RFC3339 原样保存");
        let stamped = store.add(BanEntry::new("user-2")).await.expect("写入");
        assert!(
            stamped.at.contains('T'),
            "空 at 需打上当前时间: {stamped:?}"
        );
        let again = store
            .add(BanEntry::new("user-1").with_message("改文案"))
            .await
            .expect("覆写");
        assert_eq!(again.at, "2024-05-06T07:08:09+08:00", "重封需保留原 at");
        store.remove("user-1").await.expect("解封");
        store.remove("user-2").await.expect("解封");
        assert_eq!(
            kv.raw("roster"),
            Some(serde_json::json!([])),
            "清空后仍保留空数组"
        );
    }

    #[tokio::test]
    async fn ban_save_sorts_by_openid() {
        let kv = FakeKv::shared();
        let store = BanStore::new(kv.clone());
        store
            .save(&[BanEntry::new("b"), BanEntry::new("a"), BanEntry::new("c")])
            .await
            .expect("整份写回");
        let raw = kv.raw("roster").expect("roster");
        let openids: Vec<String> = raw
            .as_array()
            .expect("数组")
            .iter()
            .map(|entry| entry["openid"].as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(openids, vec!["a", "b", "c"], "落库需按 openid 升序");
        store.clear().await.expect("清空");
        assert_eq!(store.load().await.expect("读回").len(), 0);
    }

    #[tokio::test]
    async fn panel_upsert_bumps_version_and_keeps_map_shape() {
        let kv = FakeKv::shared();
        let store = PanelStore::new(kv.clone());
        let created = store.create("group", body(0)).await.expect("新建");
        assert_eq!(created.version, 1);
        assert_eq!(created.target_type, "all");
        let again = store.create("c2c", body(0)).await.expect("新建");
        assert_ne!(again.panel_id, created.panel_id, "面板 id 需唯一");
        let map = kv.raw("menu:panel").expect("menu:panel");
        let record = &map[&created.panel_id];
        let mut keys: Vec<&str> = record
            .as_object()
            .expect("对象")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["panel", "scope", "target_type", "updated_at", "version"],
            "面板值只含约定的五个字段"
        );
        assert_eq!(record["scope"], serde_json::json!("group"));
        assert_eq!(
            record["panel"]["items"][0]["type"],
            serde_json::json!("command")
        );

        let mut edit = created.clone();
        edit.panel = body(0);
        let bumped = store.upsert(edit).await.expect("覆盖");
        assert_eq!(bumped.version, 2);
        assert_eq!(bumped.panel.version, 2);
        assert_eq!(
            store
                .get(&created.panel_id)
                .await
                .expect("查")
                .expect("存在")
                .version,
            2
        );
        assert_eq!(store.list("group").await.expect("按场景").len(), 1);
        assert_eq!(store.count().await.expect("计数"), 2);
        assert!(store.remove(&created.panel_id).await.expect("删"));
        assert!(!store.remove(&created.panel_id).await.expect("再删"));
        assert_eq!(store.count().await.expect("计数"), 1);
    }

    #[tokio::test]
    async fn panel_snapshots_are_capped_at_twenty() {
        let kv = FakeKv::shared();
        let store = PanelStore::new(kv.clone());
        let record = store.create("group", body(0)).await.expect("新建");
        let mut names = Vec::new();
        for index in 0..(PANEL_SNAPSHOT_LIMIT + 5) {
            let snapshot = store
                .push_snapshot(&record, "update")
                .await
                .expect("留快照");
            assert_eq!(snapshot.seq, index as u64 + 1);
            names.push(snapshot.name);
        }
        let snapshots = store.snapshots(&record.panel_id).await.expect("快照表");
        assert_eq!(snapshots.len(), PANEL_SNAPSHOT_LIMIT);
        assert_eq!(
            snapshots[0].seq,
            (PANEL_SNAPSHOT_LIMIT + 5) as u64,
            "最新的快照排在最前"
        );
        assert_eq!(snapshots.last().map(|item| item.seq), Some(6));
        let index = kv.raw("menu:snapshot:index").expect("索引");
        assert_eq!(
            index.as_array().map(Vec::len),
            Some(PANEL_SNAPSHOT_LIMIT),
            "索引与快照数量一致"
        );
        assert!(
            kv.raw(&snapshot_key(&record.panel_id, 1)).is_none(),
            "最旧的快照需被清掉"
        );
        assert!(kv
            .raw(&snapshot_key(
                &record.panel_id,
                (PANEL_SNAPSHOT_LIMIT + 5) as u64
            ))
            .is_some());
        let name = names.last().expect("最后一份").clone();
        let found = store.snapshot(&name).await.expect("按名查").expect("存在");
        assert_eq!(found.panel_id, record.panel_id);
    }

    #[tokio::test]
    async fn panel_restore_reapplies_snapshot_content() {
        let kv = FakeKv::shared();
        let store = PanelStore::new(kv);
        let record = store.create("group", body(0)).await.expect("新建");
        store
            .push_snapshot(&record, "update")
            .await
            .expect("留快照");
        let mut edit = record.clone();
        edit.panel.remark = "改过".to_string();
        let updated = store.upsert(edit).await.expect("覆盖");
        assert_eq!(updated.panel.remark, "改过");
        let restored = store.restore(&record.panel_id, 1).await.expect("回滚");
        assert_eq!(restored.panel.remark, "面板");
        assert_eq!(restored.version, 3, "回滚同样涨版本");
        let err = store
            .restore(&record.panel_id, 99)
            .await
            .expect_err("没有这份");
        assert_eq!(err.to_string(), "快照不存在");
    }

    #[tokio::test]
    async fn asset_provider_array_round_trips() {
        let kv = FakeKv::shared();
        let store = AssetStore::new(kv.clone());
        assert!(store.load().await.expect("读").is_empty());
        let providers = vec![
            AssetProvider {
                id: "local".to_string(),
                name: "本地".to_string(),
                kind: "local".to_string(),
                base_url: "https://cdn.example.com".to_string(),
                enabled: true,
                ..AssetProvider::default()
            },
            AssetProvider {
                id: "remote".to_string(),
                name: "远端".to_string(),
                kind: "http".to_string(),
                base_url: String::new(),
                enabled: false,
                ..AssetProvider::default()
            },
        ];
        store.save(&providers).await.expect("写");
        assert_eq!(
            kv.raw("assets:providers"),
            Some(serde_json::json!([
                {
                    "id": "local",
                    "name": "本地",
                    "kind": "local",
                    "base_url": "https://cdn.example.com",
                    "enabled": true,
                },
                {
                    "id": "remote",
                    "name": "远端",
                    "kind": "http",
                    "base_url": "",
                    "enabled": false,
                }
            ]))
        );
        assert_eq!(store.load().await.expect("读回"), providers);
        store
            .save_whitelist(&["cdn.example.com".to_string()])
            .await
            .expect("写白名单");
        assert_eq!(
            store.load_whitelist().await.expect("读白名单"),
            vec!["cdn.example.com".to_string()]
        );
    }
}
