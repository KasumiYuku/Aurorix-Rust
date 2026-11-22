use std::sync::{Arc, Mutex};

use aurorix_admin::{
    MenuSource, Panel, PanelItem, PanelRecord, PanelRemote, RemoteMenuSource, SourceError,
    SourceFuture,
};

#[derive(Default)]
struct OpLog {
    ops: Mutex<Vec<String>>,
}

impl OpLog {
    fn push(&self, op: String) {
        self.ops.lock().expect("锁调用记录").push(op);
    }

    fn all(&self) -> Vec<String> {
        self.ops.lock().expect("锁调用记录").clone()
    }

    fn remote_ops(&self) -> Vec<String> {
        self.all()
            .into_iter()
            .filter(|op| op.starts_with("remote."))
            .collect()
    }
}

struct FakeLocal {
    log: Arc<OpLog>,
    records: Vec<PanelRecord>,
    created: Mutex<usize>,
}

impl FakeLocal {
    fn new(log: Arc<OpLog>, records: Vec<PanelRecord>) -> Self {
        Self {
            log,
            records,
            created: Mutex::new(0),
        }
    }
}

impl MenuSource for FakeLocal {
    fn panels(&self, scope: &str) -> Result<Vec<PanelRecord>, String> {
        self.log.push(format!("local.panels:{scope}"));
        Ok(self
            .records
            .iter()
            .filter(|record| record.scope == scope)
            .cloned()
            .collect())
    }

    fn count(&self) -> Result<usize, String> {
        self.log.push("local.count".to_string());
        Ok(self.records.len())
    }

    fn get(&self, panel_id: &str) -> Option<PanelRecord> {
        self.log.push(format!("local.get:{panel_id}"));
        self.records
            .iter()
            .find(|record| record.panel_id == panel_id)
            .cloned()
    }

    fn create(&self, scope: &str, _panel: Panel) -> Result<String, String> {
        self.log.push(format!("local.create:{scope}"));
        let mut created = self.created.lock().expect("锁本地计数");
        *created += 1;
        let panel_id = format!("local-{}", *created);
        Ok(panel_id)
    }

    fn update(&self, panel_id: &str, _panel: Panel) -> Result<i32, String> {
        self.log.push(format!("local.update:{panel_id}"));
        Ok(2)
    }

    fn delete(&self, panel_id: &str) -> Result<(), String> {
        self.log.push(format!("local.delete:{panel_id}"));
        Ok(())
    }
}

struct FakeRemote {
    log: Arc<OpLog>,
    records: Vec<PanelRecord>,
    failure: Option<String>,
}

impl FakeRemote {
    fn new(log: Arc<OpLog>, records: Vec<PanelRecord>) -> Self {
        Self {
            log,
            records,
            failure: None,
        }
    }

    fn failing(log: Arc<OpLog>, message: &str) -> Self {
        Self {
            log,
            records: Vec::new(),
            failure: Some(message.to_string()),
        }
    }
}

impl PanelRemote for FakeRemote {
    fn list(&self, scope: String) -> SourceFuture<'_, Result<Vec<PanelRecord>, SourceError>> {
        Box::pin(async move {
            self.log.push(format!("remote.list:{scope}"));
            Ok(self
                .records
                .iter()
                .filter(|record| record.scope == scope)
                .cloned()
                .collect())
        })
    }

    fn count(&self) -> SourceFuture<'_, Result<usize, SourceError>> {
        Box::pin(async move {
            self.log.push("remote.count".to_string());
            Ok(self.records.len())
        })
    }

    fn get(&self, panel_id: String) -> SourceFuture<'_, Result<Option<PanelRecord>, SourceError>> {
        Box::pin(async move {
            self.log.push(format!("remote.get:{panel_id}"));
            Ok(self
                .records
                .iter()
                .find(|record| record.panel_id == panel_id)
                .cloned())
        })
    }

    fn create(&self, scope: String, panel: Panel) -> SourceFuture<'_, Result<String, SourceError>> {
        Box::pin(async move {
            self.log
                .push(format!("remote.create:{scope}:{}", panel.items.len()));
            if let Some(message) = self.failure.as_ref() {
                return Err(SourceError::Server(message.clone()));
            }
            Ok("remote-1".to_string())
        })
    }

    fn update(
        &self,
        panel_id: String,
        _panel: Panel,
    ) -> SourceFuture<'_, Result<i32, SourceError>> {
        Box::pin(async move {
            self.log.push(format!("remote.update:{panel_id}"));
            if let Some(message) = self.failure.as_ref() {
                return Err(SourceError::Server(message.clone()));
            }
            Ok(3)
        })
    }

    fn delete(&self, panel_id: String) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move {
            self.log.push(format!("remote.delete:{panel_id}"));
            if let Some(message) = self.failure.as_ref() {
                return Err(SourceError::Server(message.clone()));
            }
            Ok(())
        })
    }
}

fn item(name: &str, desc: &str) -> PanelItem {
    PanelItem {
        name: name.to_string(),
        desc: desc.to_string(),
        kind: "command".to_string(),
        only_admin: false,
        link: String::new(),
    }
}

fn panel(entries: Vec<PanelItem>) -> Panel {
    Panel {
        items: entries,
        remark: String::new(),
        version: 0,
    }
}

fn record(panel_id: &str, scope: &str, panel: Panel) -> PanelRecord {
    PanelRecord {
        panel_id: panel_id.to_string(),
        scope: scope.to_string(),
        target_type: "all".to_string(),
        panel,
        ..PanelRecord::default()
    }
}

#[tokio::test]
async fn case_003() {
    let log = Arc::new(OpLog::default());
    let local = Arc::new(FakeLocal::new(Arc::clone(&log), Vec::new()));
    let source = RemoteMenuSource::new(local);

    assert!(!source.remote_enabled(), "未挂载远程时不应报告远程已启用");

    let panel_id = source
        .create_async("group".to_string(), panel(vec![item("签到", "每日签到")]))
        .await
        .expect("本地新建面板应当成功");

    assert_eq!(panel_id, "local-1", "新建面板应返回本地生成的 id");
    assert_eq!(
        log.all(),
        vec!["local.create:group".to_string()],
        "未启用远程时只应调用本地"
    );
    assert!(
        log.remote_ops().is_empty(),
        "未启用远程时不应有任何远程调用, 实际: {:?}",
        log.remote_ops()
    );
}

#[tokio::test]
async fn case_004() {
    let log = Arc::new(OpLog::default());
    let local = Arc::new(FakeLocal::new(Arc::clone(&log), Vec::new()));
    let remote = Arc::new(FakeRemote::new(Arc::clone(&log), Vec::new()));
    let source = RemoteMenuSource::new(local).with_remote(remote);

    assert!(source.remote_enabled(), "挂载远程后应报告远程已启用");

    let panel_id = source
        .create_async("group".to_string(), panel(vec![item("签到", "每日签到")]))
        .await
        .expect("本地与远程都成功时应返回成功");

    assert_eq!(
        log.all(),
        vec![
            "local.create:group".to_string(),
            "remote.create:group:1".to_string()
        ],
        "应当先写本地再镜像到远程, 实际: {:?}",
        log.all()
    );
    assert_eq!(
        panel_id, "local-1",
        "返回值应当是本地生成的 id, 而不是远程分配的 id"
    );
}

#[tokio::test]
async fn case_005() {
    let log = Arc::new(OpLog::default());
    let local = Arc::new(FakeLocal::new(Arc::clone(&log), Vec::new()));
    let remote = Arc::new(FakeRemote::failing(Arc::clone(&log), "QQ 接口超时"));
    let source = RemoteMenuSource::new(local).with_remote(remote);

    let err = source
        .update_async("local-1".to_string(), panel(vec![item("签到", "每日签到")]))
        .await
        .expect_err("远程覆盖失败时应当返回错误");

    assert!(
        err.message().contains("本地已保存"),
        "错误提示应当说明本地已保存, 实际: {}",
        err.message()
    );
    assert!(
        err.message().contains("QQ 接口超时"),
        "错误提示应当带上远程失败原因, 实际: {}",
        err.message()
    );
    assert_eq!(
        log.all(),
        vec![
            "local.update:local-1".to_string(),
            "remote.update:local-1".to_string()
        ],
        "应当先写本地再用同一个 id 镜像远程, 实际: {:?}",
        log.all()
    );
}

#[tokio::test]
async fn case_006() {
    let log = Arc::new(OpLog::default());
    let local = Arc::new(FakeLocal::new(Arc::clone(&log), Vec::new()));
    let remote = Arc::new(FakeRemote::new(Arc::clone(&log), Vec::new()));
    let source = RemoteMenuSource::new(local).with_remote(remote);

    source
        .delete_async("local-7".to_string())
        .await
        .expect("本地与远程都成功时应当返回成功");

    assert_eq!(
        log.all(),
        vec![
            "local.delete:local-7".to_string(),
            "remote.delete:local-7".to_string()
        ],
        "删除应当先删本地再删远程, 实际: {:?}",
        log.all()
    );
}

#[tokio::test]
async fn case_007() {
    let log = Arc::new(OpLog::default());
    let locals = vec![
        record("p1", "group", panel(vec![item("签到", "每日签到")])),
        record("p2", "group", panel(vec![item("帮助", "查看帮助")])),
    ];
    let remotes = vec![
        record("p2", "group", panel(vec![item("帮助", "旧的帮助说明")])),
        record("p3", "group", panel(vec![item("排行", "查看排行")])),
    ];
    let local = Arc::new(FakeLocal::new(Arc::clone(&log), locals));
    let remote = Arc::new(FakeRemote::new(Arc::clone(&log), remotes));
    let source = RemoteMenuSource::new(local).with_remote(remote);

    let report = source.sync_all_async().await.expect("对账应当成功");

    assert_eq!(report.created, 1, "应当只新建 1 个面板");
    assert_eq!(report.updated, 1, "应当只更新 1 个面板");
    assert_eq!(report.deleted, 1, "应当只删除 1 个面板");
    assert_eq!(report.failed, 0, "不应有失败的操作");
    assert!(report.errors.is_empty(), "不应记录失败原因");
    assert_eq!(report.scopes, 4, "应当遍历四个场景");
    assert_eq!(
        log.remote_ops(),
        vec![
            "remote.list:group".to_string(),
            "remote.create:group:1".to_string(),
            "remote.update:p2".to_string(),
            "remote.delete:p3".to_string(),
            "remote.list:c2c".to_string(),
            "remote.list:channel".to_string(),
            "remote.list:dm".to_string(),
        ],
        "对账应当按场景先读远程再做增删改, 实际: {:?}",
        log.remote_ops()
    );
}

#[tokio::test]
async fn case_008() {
    let log = Arc::new(OpLog::default());
    let records = vec![record("p1", "group", panel(vec![item("签到", "每日签到")]))];
    let local = Arc::new(FakeLocal::new(Arc::clone(&log), records));
    let remote = Arc::new(FakeRemote::new(Arc::clone(&log), Vec::new()));
    let source = RemoteMenuSource::new(local).with_remote(remote);

    let panels = source
        .panels_async("group".to_string())
        .await
        .expect("读取面板应当成功");
    let count = source.count_async().await.expect("读取数量应当成功");
    let found = source
        .get_async("p1".to_string())
        .await
        .expect("读取单个面板应当成功");

    assert_eq!(panels.len(), 1, "应当读到 1 个面板");
    assert_eq!(count, 1, "数量应当是 1");
    assert!(found.is_some(), "应当能按 id 找到面板");
    assert!(
        log.remote_ops().is_empty(),
        "读操作不应触发远程调用, 实际: {:?}",
        log.remote_ops()
    );
}

#[tokio::test]
async fn case_009() {
    let log = Arc::new(OpLog::default());
    let local = Arc::new(FakeLocal::new(
        Arc::clone(&log),
        vec![record("p1", "group", panel(vec![item("签到", "每日签到")]))],
    ));
    let source = RemoteMenuSource::new(local);

    let report = source
        .sync_all_async()
        .await
        .expect("未启用远程时对账应当成功");

    assert_eq!(report.created, 0, "未启用远程时不应新建");
    assert_eq!(report.updated, 0, "未启用远程时不应更新");
    assert_eq!(report.deleted, 0, "未启用远程时不应删除");
    assert_eq!(report.failed, 0, "未启用远程时不应有失败");
    assert_eq!(report.scopes, 0, "未启用远程时不应遍历场景");
    assert!(!source.remote_enabled(), "未挂载远程时远程未启用");
}

#[tokio::test]
async fn case_010() {
    let log = Arc::new(OpLog::default());
    let shared = panel(vec![item("签到", "每日签到")]);
    let local = Arc::new(FakeLocal::new(
        Arc::clone(&log),
        vec![record("local-1", "group", shared.clone())],
    ));
    let remote = Arc::new(FakeRemote::new(
        Arc::clone(&log),
        vec![record("qq-9", "group", shared.clone())],
    ));
    let source = RemoteMenuSource::new(local).with_remote(remote);

    let report = source.sync_all_async().await.expect("对账应当成功");

    assert_eq!(report.created, 0, "内容相同不该重复新建");
    assert_eq!(report.updated, 0, "内容相同不该触发更新");
    assert_eq!(report.deleted, 0, "内容相同不该删除远程面板");
    assert!(report.failed == 0, "对账不该有失败: {:?}", report.errors);
}
