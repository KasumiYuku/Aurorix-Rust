use std::sync::Arc;

use aurorix_log::Logger;
use serde::Serialize;

use crate::menu::{MenuSnapshot, MenuSource, Panel, PanelRecord, PANEL_SCOPES};
use crate::sources::{SourceError, SourceFuture};

const MODULE: &str = "admin.panels";

const MAX_SYNC_ERRORS: usize = 20;

pub trait PanelRemote: Send + Sync {
    fn list(&self, scope: String) -> SourceFuture<'_, Result<Vec<PanelRecord>, SourceError>>;

    fn count(&self) -> SourceFuture<'_, Result<usize, SourceError>>;

    fn get(&self, panel_id: String) -> SourceFuture<'_, Result<Option<PanelRecord>, SourceError>>;

    fn create(&self, scope: String, panel: Panel) -> SourceFuture<'_, Result<String, SourceError>>;

    fn update(&self, panel_id: String, panel: Panel) -> SourceFuture<'_, Result<i32, SourceError>>;

    fn delete(&self, panel_id: String) -> SourceFuture<'_, Result<(), SourceError>>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PanelSyncReport {
    pub scopes: usize,
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

pub struct RemoteMenuSource {
    local: Arc<dyn MenuSource>,
    remote: Option<Arc<dyn PanelRemote>>,
    logger: Option<Arc<Logger>>,
}

impl RemoteMenuSource {
    pub fn new(local: Arc<dyn MenuSource>) -> Self {
        Self {
            local,
            remote: None,
            logger: None,
        }
    }

    pub fn with_remote(mut self, remote: Arc<dyn PanelRemote>) -> Self {
        self.remote = Some(remote);
        self
    }

    pub fn with_logger(mut self, logger: Arc<Logger>) -> Self {
        self.logger = Some(logger);
        self
    }

    pub fn remote_enabled(&self) -> bool {
        self.remote.is_some()
    }

    pub fn sync_all_async(&self) -> SourceFuture<'_, Result<PanelSyncReport, SourceError>> {
        Box::pin(async move {
            let Some(remote) = self.remote.as_deref() else {
                return Ok(PanelSyncReport::default());
            };
            let mut report = PanelSyncReport::default();
            for (scope, label) in PANEL_SCOPES {
                report.scopes += 1;
                let locals = match self.local.panels_async(scope.to_string()).await {
                    Ok(records) => records,
                    Err(err) => {
                        push_error(
                            &mut report,
                            format!("读取本地{label}面板失败: {}", err.message()),
                        );
                        continue;
                    }
                };
                let existing = match remote.list(scope.to_string()).await {
                    Ok(records) => records,
                    Err(err) => {
                        push_error(
                            &mut report,
                            format!("读取远程{label}面板失败: {}", err.message()),
                        );
                        continue;
                    }
                };
                let mut claimed = vec![false; existing.len()];
                for record in &locals {
                    let by_id = existing
                        .iter()
                        .position(|item| item.panel_id == record.panel_id);
                    let index = match by_id {
                        Some(index) => Some(index),
                        None => existing
                            .iter()
                            .enumerate()
                            .find(|(index, item)| {
                                !claimed[*index] && same_content(&record.panel, &item.panel)
                            })
                            .map(|(index, _)| index),
                    };
                    let Some(index) = index else {
                        match remote.create(scope.to_string(), record.panel.clone()).await {
                            Ok(remote_id) => {
                                report.created += 1;
                                self.debug(&format!(
                                    "远程新建{label}面板: 本地 id={} 远程 id={remote_id}",
                                    record.panel_id
                                ));
                            }
                            Err(err) => push_error(
                                &mut report,
                                format!(
                                    "新建{label}面板 {} 失败: {}",
                                    record.panel_id,
                                    err.message()
                                ),
                            ),
                        }
                        continue;
                    };
                    claimed[index] = true;
                    if same_content(&record.panel, &existing[index].panel) {
                        continue;
                    }
                    let remote_id = existing[index].panel_id.clone();
                    match remote.update(remote_id.clone(), record.panel.clone()).await {
                        Ok(_) => report.updated += 1,
                        Err(err) => push_error(
                            &mut report,
                            format!("更新{label}面板 {remote_id} 失败: {}", err.message()),
                        ),
                    }
                }
                for (index, found) in existing.iter().enumerate() {
                    if claimed[index] {
                        continue;
                    }
                    match remote.delete(found.panel_id.clone()).await {
                        Ok(()) => report.deleted += 1,
                        Err(err) => push_error(
                            &mut report,
                            format!("删除{label}面板 {} 失败: {}", found.panel_id, err.message()),
                        ),
                    }
                }
            }
            self.info(&format!(
                "同步 QQ 面板完成: 场景={} 新建={} 更新={} 删除={} 失败={}",
                report.scopes, report.created, report.updated, report.deleted, report.failed
            ));
            Ok(report)
        })
    }

    fn mirror_failure(err: &SourceError) -> SourceError {
        SourceError::Server(format!(
            "本地已保存, 但同步到 QQ 面板失败: {}",
            err.message()
        ))
    }

    fn debug(&self, message: &str) {
        if let Some(logger) = self.logger.as_ref() {
            logger.debug(MODULE, message);
        }
    }

    fn info(&self, message: &str) {
        if let Some(logger) = self.logger.as_ref() {
            logger.info(MODULE, message);
        }
    }

    fn warn(&self, message: &str) {
        if let Some(logger) = self.logger.as_ref() {
            logger.warn(MODULE, message);
        }
    }
}

fn same_content(local: &Panel, remote: &Panel) -> bool {
    local.remark == remote.remark && local.items == remote.items
}

fn push_error(report: &mut PanelSyncReport, message: String) {
    report.failed += 1;
    if report.errors.len() < MAX_SYNC_ERRORS {
        report.errors.push(message);
    }
}

impl MenuSource for RemoteMenuSource {
    fn sync_async(&self) -> SourceFuture<'_, Result<PanelSyncReport, SourceError>> {
        self.sync_all_async()
    }

    fn panels(&self, scope: &str) -> Result<Vec<PanelRecord>, String> {
        self.local.panels(scope)
    }

    fn count(&self) -> Result<usize, String> {
        self.local.count()
    }

    fn get(&self, panel_id: &str) -> Option<PanelRecord> {
        self.local.get(panel_id)
    }

    fn create(&self, scope: &str, panel: Panel) -> Result<String, String> {
        let panel_id = self.local.create(scope, panel)?;
        self.debug(&format!(
            "新建指令面板 {panel_id} 只写本地, 镜像到 QQ 面板走异步路径"
        ));
        Ok(panel_id)
    }

    fn update(&self, panel_id: &str, panel: Panel) -> Result<i32, String> {
        let version = self.local.update(panel_id, panel)?;
        self.debug(&format!(
            "覆盖指令面板 {panel_id} 只写本地, 镜像到 QQ 面板走异步路径"
        ));
        Ok(version)
    }

    fn delete(&self, panel_id: &str) -> Result<(), String> {
        self.local.delete(panel_id)?;
        self.debug(&format!(
            "删除指令面板 {panel_id} 只写本地, 镜像到 QQ 面板走异步路径"
        ));
        Ok(())
    }

    fn snapshots(&self) -> Vec<MenuSnapshot> {
        self.local.snapshots()
    }

    fn snapshot_dir(&self) -> String {
        self.local.snapshot_dir()
    }

    fn save_snapshot(&self, action: &str, record: &PanelRecord) -> Result<String, String> {
        self.local.save_snapshot(action, record)
    }

    fn read_snapshot(&self, name: &str) -> Option<MenuSnapshot> {
        self.local.read_snapshot(name)
    }

    fn panels_async(
        &self,
        scope: String,
    ) -> SourceFuture<'_, Result<Vec<PanelRecord>, SourceError>> {
        self.local.panels_async(scope)
    }

    fn count_async(&self) -> SourceFuture<'_, Result<usize, SourceError>> {
        self.local.count_async()
    }

    fn get_async(
        &self,
        panel_id: String,
    ) -> SourceFuture<'_, Result<Option<PanelRecord>, SourceError>> {
        self.local.get_async(panel_id)
    }

    fn create_async(
        &self,
        scope: String,
        panel: Panel,
    ) -> SourceFuture<'_, Result<String, SourceError>> {
        Box::pin(async move {
            let panel_id = self
                .local
                .create_async(scope.clone(), panel.clone())
                .await?;
            let Some(remote) = self.remote.as_deref() else {
                return Ok(panel_id);
            };
            if let Err(err) = remote.create(scope, panel).await {
                self.warn(&format!(
                    "新建指令面板 {panel_id} 已写本地, 同步到 QQ 面板失败: {}",
                    err.message()
                ));
                return Err(Self::mirror_failure(&err));
            }
            Ok(panel_id)
        })
    }

    fn update_async(
        &self,
        panel_id: String,
        panel: Panel,
    ) -> SourceFuture<'_, Result<i32, SourceError>> {
        Box::pin(async move {
            let version = self
                .local
                .update_async(panel_id.clone(), panel.clone())
                .await?;
            let Some(remote) = self.remote.as_deref() else {
                return Ok(version);
            };
            if let Err(err) = remote.update(panel_id.clone(), panel).await {
                self.warn(&format!(
                    "覆盖指令面板 {panel_id} 已写本地, 同步到 QQ 面板失败: {}",
                    err.message()
                ));
                return Err(Self::mirror_failure(&err));
            }
            Ok(version)
        })
    }

    fn delete_async(&self, panel_id: String) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.local.delete_async(panel_id.clone()).await?;
            let Some(remote) = self.remote.as_deref() else {
                return Ok(());
            };
            if let Err(err) = remote.delete(panel_id.clone()).await {
                self.warn(&format!(
                    "删除指令面板 {panel_id} 已写本地, 同步到 QQ 面板失败: {}",
                    err.message()
                ));
                return Err(Self::mirror_failure(&err));
            }
            Ok(())
        })
    }

    fn snapshots_async(&self) -> SourceFuture<'_, Result<Vec<MenuSnapshot>, SourceError>> {
        self.local.snapshots_async()
    }

    fn save_snapshot_async(
        &self,
        action: String,
        record: PanelRecord,
    ) -> SourceFuture<'_, Result<String, SourceError>> {
        self.local.save_snapshot_async(action, record)
    }

    fn read_snapshot_async(
        &self,
        name: String,
    ) -> SourceFuture<'_, Result<Option<MenuSnapshot>, SourceError>> {
        self.local.read_snapshot_async(name)
    }
}
