use std::collections::BTreeMap;
use std::sync::Arc;

use aurorix_assets::registry::{ConfigField as ProviderField, ProviderRegistry};
use aurorix_core::Target;
use aurorix_db::admin::{
    AssetProvider as DbAssetProvider, BanEntry as DbBanEntry, PanelBody, PanelItem as DbPanelItem,
    PanelRecord as StoredPanel,
};
use aurorix_db::{AssetStore, BanStore, Db, InboxStore, PanelSnapshot, PanelStore};
use serde_json::{Map, Value};

use crate::assets::{AssetField, AssetProvider, AssetReload, AssetSource, AssetsInput, AssetsView};
use crate::bans::{BanEntry, BanSource};
use crate::inbox::{Conversation, InboxMessage, InboxSendError, InboxSender, InboxSource};
use crate::menu::{MenuSnapshot, MenuSource, Panel, PanelItem, PanelRecord, RestoredPanel};
use crate::sources::{SourceError, SourceFuture};

#[derive(Debug, Clone)]
pub struct PgBans {
    store: BanStore,
}

impl PgBans {
    pub fn new(store: BanStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &BanStore {
        &self.store
    }
}

fn from_db_ban(entry: DbBanEntry) -> BanEntry {
    BanEntry {
        openid: entry.openid,
        message: entry.message,
        note: entry.note,
        at: entry.at,
    }
}

fn to_db_ban(entry: BanEntry) -> DbBanEntry {
    DbBanEntry {
        openid: entry.openid,
        message: entry.message,
        note: entry.note,
        at: entry.at,
    }
}

impl BanSource for PgBans {
    fn list_async(&self) -> SourceFuture<'_, Result<Vec<BanEntry>, SourceError>> {
        Box::pin(async move {
            let entries = self.store.load().await.map_err(SourceError::from_db)?;
            Ok(entries.into_iter().map(from_db_ban).collect())
        })
    }

    fn put_async(&self, entry: BanEntry) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.store
                .add(to_db_ban(entry))
                .await
                .map_err(SourceError::from_db)?;
            Ok(())
        })
    }

    fn delete_async(&self, openid: String) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.store
                .remove(&openid)
                .await
                .map_err(SourceError::from_db)?;
            Ok(())
        })
    }
}

fn to_db_item(item: PanelItem) -> DbPanelItem {
    DbPanelItem {
        name: item.name,
        desc: item.desc,
        kind: item.kind,
        only_admin: item.only_admin,
        link: item.link,
    }
}

fn from_db_item(item: DbPanelItem) -> PanelItem {
    PanelItem {
        name: item.name,
        desc: item.desc,
        kind: item.kind,
        only_admin: item.only_admin,
        link: item.link,
    }
}

fn to_db_body(panel: Panel) -> PanelBody {
    PanelBody {
        items: panel.items.into_iter().map(to_db_item).collect(),
        remark: panel.remark,
        version: panel.version,
    }
}

fn from_db_body(body: PanelBody) -> Panel {
    Panel {
        items: body.items.into_iter().map(from_db_item).collect(),
        remark: body.remark,
        version: body.version,
    }
}

fn from_db_panel(record: StoredPanel) -> PanelRecord {
    PanelRecord {
        panel_id: record.panel_id,
        scope: record.scope,
        target_type: record.target_type,
        panel: from_db_body(record.panel),
        created_at: record.updated_at.clone(),
        updated_at: record.updated_at,
        version: record.version,
        user_openids: Vec::new(),
        group_openids: Vec::new(),
    }
}

fn to_db_panel(record: PanelRecord) -> StoredPanel {
    StoredPanel {
        panel_id: record.panel_id,
        scope: record.scope,
        target_type: record.target_type,
        panel: to_db_body(record.panel),
        version: record.version,
        updated_at: record.updated_at,
    }
}

fn from_db_snapshot(snapshot: PanelSnapshot) -> MenuSnapshot {
    MenuSnapshot {
        file: snapshot.name,
        panel_id: snapshot.panel_id,
        scope: snapshot.record.scope.clone(),
        target_type: snapshot.record.target_type.clone(),
        panel: from_db_body(snapshot.record.panel),
        version: snapshot.record.version,
        action: snapshot.action,
        saved_at: snapshot.saved_at,
    }
}

#[derive(Debug, Clone)]
pub struct PgMenu {
    store: PanelStore,
}

impl PgMenu {
    pub fn new(store: PanelStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &PanelStore {
        &self.store
    }
}

impl MenuSource for PgMenu {
    fn panels_async(
        &self,
        scope: String,
    ) -> SourceFuture<'_, Result<Vec<PanelRecord>, SourceError>> {
        Box::pin(async move {
            let records = self
                .store
                .list(&scope)
                .await
                .map_err(SourceError::from_db)?;
            Ok(records.into_iter().map(from_db_panel).collect())
        })
    }

    fn count_async(&self) -> SourceFuture<'_, Result<usize, SourceError>> {
        Box::pin(async move { self.store.count().await.map_err(SourceError::from_db) })
    }

    fn get_async(
        &self,
        panel_id: String,
    ) -> SourceFuture<'_, Result<Option<PanelRecord>, SourceError>> {
        Box::pin(async move {
            let record = self
                .store
                .get(&panel_id)
                .await
                .map_err(SourceError::from_db)?;
            Ok(record.map(from_db_panel))
        })
    }

    fn create_async(
        &self,
        scope: String,
        panel: Panel,
    ) -> SourceFuture<'_, Result<String, SourceError>> {
        Box::pin(async move {
            let record = self
                .store
                .create(&scope, to_db_body(panel))
                .await
                .map_err(SourceError::from_db)?;
            Ok(record.panel_id)
        })
    }

    fn update_async(
        &self,
        panel_id: String,
        panel: Panel,
    ) -> SourceFuture<'_, Result<i32, SourceError>> {
        Box::pin(async move {
            let Some(mut record) = self
                .store
                .get(&panel_id)
                .await
                .map_err(SourceError::from_db)?
            else {
                return Err(SourceError::NotFound("面板不存在".to_string()));
            };
            record.panel = to_db_body(panel);
            let record = self
                .store
                .upsert(record)
                .await
                .map_err(SourceError::from_db)?;
            Ok(record.version)
        })
    }

    fn delete_async(&self, panel_id: String) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            let removed = self
                .store
                .remove(&panel_id)
                .await
                .map_err(SourceError::from_db)?;
            if !removed {
                return Err(SourceError::NotFound("面板不存在".to_string()));
            }
            Ok(())
        })
    }

    fn snapshots_async(&self) -> SourceFuture<'_, Result<Vec<MenuSnapshot>, SourceError>> {
        Box::pin(async move {
            let snapshots = self
                .store
                .all_snapshots()
                .await
                .map_err(SourceError::from_db)?;
            Ok(snapshots.into_iter().map(from_db_snapshot).collect())
        })
    }

    fn save_snapshot_async(
        &self,
        action: String,
        record: PanelRecord,
    ) -> SourceFuture<'_, Result<String, SourceError>> {
        Box::pin(async move {
            let stored = to_db_panel(record);
            let snapshot = self
                .store
                .push_snapshot(&stored, &action)
                .await
                .map_err(SourceError::from_db)?;
            Ok(snapshot.name)
        })
    }

    fn read_snapshot_async(
        &self,
        name: String,
    ) -> SourceFuture<'_, Result<Option<MenuSnapshot>, SourceError>> {
        Box::pin(async move {
            let snapshot = self
                .store
                .snapshot(&name)
                .await
                .map_err(SourceError::from_db)?;
            Ok(snapshot.map(from_db_snapshot))
        })
    }

    fn restore_async(
        &self,
        name: String,
    ) -> SourceFuture<'_, Result<Option<RestoredPanel>, SourceError>> {
        Box::pin(async move {
            let Some(snapshot) = self
                .store
                .snapshot(&name)
                .await
                .map_err(SourceError::from_db)?
            else {
                return Ok(None);
            };
            let existed = self
                .store
                .get(&snapshot.panel_id)
                .await
                .map_err(SourceError::from_db)?
                .is_some();
            let record = self
                .store
                .restore(&snapshot.panel_id, snapshot.seq)
                .await
                .map_err(SourceError::from_db)?;
            Ok(Some(RestoredPanel {
                panel_id: record.panel_id,
                scope: record.scope,
                mode: if existed { "overwrite" } else { "recreated" }.to_string(),
            }))
        })
    }
}

fn config_text(config: &Map<String, Value>, key: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            continue;
        }
        if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        return "provider".to_string();
    }
    trimmed
}

fn view_field(field: &ProviderField) -> AssetField {
    AssetField {
        key: field.key.clone(),
        label: field.label.clone(),
        description: field.description.clone(),
        kind: field.field_type.clone(),
        required: field.required,
        placeholder: field.placeholder.clone(),
        default: field.default_value.clone(),
        options: field.options.clone(),
    }
}

fn secret_filled(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

fn masked_config(fields: &[ProviderField], config: &Map<String, Value>) -> (Map<String, Value>, bool) {
    let mut out = config.clone();
    let mut has_secrets = false;
    for field in fields.iter().filter(|field| field.field_type == "password") {
        has_secrets |= secret_filled(out.get(&field.key));
        out.insert(field.key.clone(), Value::String(String::new()));
    }
    (out, has_secrets)
}

fn merged_config(
    fields: &[ProviderField],
    previous: Option<&Map<String, Value>>,
    incoming: &Map<String, Value>,
) -> Map<String, Value> {
    let mut out = incoming.clone();
    for field in fields.iter().filter(|field| field.field_type == "password") {
        if secret_filled(out.get(&field.key)) {
            continue;
        }
        if let Some(value) = previous.and_then(|old| old.get(&field.key).cloned()) {
            out.insert(field.key.clone(), value);
        }
    }
    out
}

fn to_db_providers(
    input: AssetsInput,
    registry: &ProviderRegistry,
    previous: &[DbAssetProvider],
) -> Vec<DbAssetProvider> {
    let old: BTreeMap<&str, &Map<String, Value>> = previous
        .iter()
        .map(|item| (item.name.as_str(), &item.config))
        .collect();
    let mut providers = input.providers;
    providers.sort_by_key(|item| item.priority);
    providers
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let fields = registry.schema(item.name.as_str());
            let config = merged_config(&fields, old.get(item.name.as_str()).copied(), &item.config);
            let id = config_text(&config, "id");
            DbAssetProvider {
                id: if id.is_empty() {
                    fallback_id(&item.name, index)
                } else {
                    id
                },
                name: item.name,
                kind: config_text(&config, "kind"),
                base_url: config_text(&config, "base_url"),
                enabled: item.enabled,
                priority: i32::try_from(item.priority).unwrap_or(0),
                probe: None,
                config,
            }
        })
        .collect()
}

#[derive(Clone)]
pub struct PgAssets {
    store: AssetStore,
    registry: Arc<ProviderRegistry>,
    reload: Option<Arc<dyn AssetReload>>,
}

impl std::fmt::Debug for PgAssets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PgAssets")
            .field("providers", &self.registry.len())
            .finish()
    }
}

impl PgAssets {
    pub fn new(store: AssetStore) -> Self {
        Self {
            store,
            registry: Arc::new(ProviderRegistry::with_builtin()),
            reload: None,
        }
    }

    pub fn with_registry(mut self, registry: Arc<ProviderRegistry>) -> Self {
        self.registry = registry;
        self
    }

    pub fn with_reload(mut self, reload: Arc<dyn AssetReload>) -> Self {
        self.reload = Some(reload);
        self
    }

    pub fn store(&self) -> &AssetStore {
        &self.store
    }

    fn view_of(&self, stored: &[DbAssetProvider], whitelist: Vec<String>) -> AssetsView {
        let mut by_name: BTreeMap<&str, &DbAssetProvider> = BTreeMap::new();
        for item in stored {
            let name = match item.name.is_empty() {
                true => item.kind.as_str(),
                false => item.name.as_str(),
            };
            if !name.is_empty() {
                by_name.insert(name, item);
            }
        }
        let providers: Vec<AssetProvider> = self
            .registry
            .specs()
            .into_iter()
            .map(|spec| {
                let item = by_name.get(spec.name.as_str()).copied();
                let (config, has_secrets) = match item {
                    Some(item) => masked_config(&spec.fields, &item.config),
                    None => (Map::new(), false),
                };
                AssetProvider {
                    name: spec.name.clone(),
                    enabled: match item {
                        Some(item) => item.enabled,
                        None => spec.default_on,
                    },
                    priority: item.map(|item| i64::from(item.priority)).unwrap_or_default(),
                    configured: item.is_some(),
                    has_secrets,
                    schema: spec.fields.iter().map(view_field).collect(),
                    config,
                }
            })
            .collect();
        AssetsView {
            whitelist,
            providers,
        }
    }
}

fn fallback_id(name: &str, index: usize) -> String {
    let slugged = slug(name);
    if slugged == "provider" {
        return format!("provider-{}", index + 1);
    }
    slugged
}

impl AssetSource for PgAssets {
    fn view_async(&self) -> SourceFuture<'_, Result<AssetsView, SourceError>> {
        Box::pin(async move {
            let stored = self.store.load().await.map_err(SourceError::from_db)?;
            let whitelist = self
                .store
                .load_whitelist()
                .await
                .map_err(SourceError::from_db)?;
            Ok(self.view_of(&stored, whitelist))
        })
    }

    fn save_async(&self, input: AssetsInput) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            let previous = self.store.load().await.map_err(SourceError::from_db)?;
            let providers = to_db_providers(input.clone(), self.registry.as_ref(), &previous);
            self.store
                .save(&providers)
                .await
                .map_err(SourceError::from_db)?;
            self.store
                .save_whitelist(&input.whitelist)
                .await
                .map_err(SourceError::from_db)
        })
    }

    fn reload_async(&self) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            match self.reload.as_ref() {
                Some(reload) => reload.reload().await,
                None => Ok(()),
            }
        })
    }
}

fn from_db_conversation(conversation: aurorix_db::Conversation) -> Conversation {
    Conversation {
        id: conversation.id,
        kind: conversation.kind.as_str().to_string(),
        name: conversation.name,
        last_at: conversation
            .last_at
            .map(|at| at.to_rfc3339())
            .unwrap_or_default(),
        last_preview: conversation.last_preview,
    }
}

fn from_db_message(message: aurorix_db::Message) -> InboxMessage {
    InboxMessage {
        id: message.id,
        conv_id: message.conv_id,
        kind: message.kind.as_str().to_string(),
        direction: message.direction.as_str().to_string(),
        sender_id: message.sender_id,
        sender_name: message.sender_name,
        content: message.content,
        platform_id: message.platform_id,
        created_at: message.created_at.to_rfc3339(),
    }
}

#[derive(Clone)]
pub struct PgInbox {
    store: InboxStore,
    sender: Option<Arc<dyn InboxSender>>,
}

impl std::fmt::Debug for PgInbox {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PgInbox")
            .field("sender", &self.sender.is_some())
            .finish()
    }
}

impl PgInbox {
    pub fn new(store: InboxStore) -> Self {
        Self {
            store,
            sender: None,
        }
    }

    pub fn with_sender(store: InboxStore, sender: Arc<dyn InboxSender>) -> Self {
        Self {
            store,
            sender: Some(sender),
        }
    }

    pub fn store(&self) -> &InboxStore {
        &self.store
    }

    pub fn has_sender(&self) -> bool {
        self.sender.is_some()
    }
}

impl InboxSource for PgInbox {
    fn conversations_async(
        &self,
        limit: usize,
    ) -> SourceFuture<'_, Result<Vec<Conversation>, SourceError>> {
        Box::pin(async move {
            let rows = self
                .store
                .conversations_page(limit as i64, 0)
                .await
                .map_err(SourceError::from_db)?;
            Ok(rows.into_iter().map(from_db_conversation).collect())
        })
    }

    fn messages_async(
        &self,
        conv_id: String,
        before: Option<i64>,
        limit: usize,
    ) -> SourceFuture<'_, Result<Vec<InboxMessage>, SourceError>> {
        Box::pin(async move {
            let rows = self
                .store
                .messages(&conv_id, before.unwrap_or(0), limit as i64)
                .await
                .map_err(SourceError::from_db)?;
            Ok(rows.into_iter().map(from_db_message).collect())
        })
    }

    fn lookup_async(
        &self,
        conv_id: String,
    ) -> SourceFuture<'_, Result<Option<Conversation>, SourceError>> {
        Box::pin(async move {
            let found = self
                .store
                .lookup(&conv_id)
                .await
                .map_err(SourceError::from_db)?;
            Ok(found.map(from_db_conversation))
        })
    }

    fn send_async<'a>(
        &'a self,
        conv: &'a Conversation,
        text: &'a str,
    ) -> SourceFuture<'a, Result<(), InboxSendError>> {
        Box::pin(async move {
            let Some(sender) = self.sender.as_ref() else {
                return Err(InboxSendError::NotConnected);
            };
            let stored = self
                .store
                .lookup(&conv.id)
                .await
                .map_err(|error| InboxSendError::Failed(error.to_string()))?
                .ok_or_else(|| InboxSendError::Failed(String::from("会话不存在")))?;
            let target = Target::new(stored.kind, stored.id.clone());
            sender.send(&target, text).await?;
            let message = aurorix_db::Message {
                id: 0,
                conv_id: stored.id.clone(),
                kind: stored.kind,
                direction: aurorix_db::Direction::Outbound,
                sender_id: String::new(),
                sender_name: String::new(),
                content: text.to_string(),
                platform_id: String::new(),
                created_at: chrono::Utc::now(),
            };
            self.store
                .save(&message)
                .await
                .map_err(|error| InboxSendError::Failed(error.to_string()))?;
            Ok(())
        })
    }
}

pub fn postgres_sources(db: &Arc<Db>) -> (PgBans, PgMenu, PgAssets, PgInbox) {
    (
        PgBans::new(db.bans()),
        PgMenu::new(db.panels()),
        PgAssets::new(db.assets()),
        PgInbox::new(db.inbox()),
    )
}
