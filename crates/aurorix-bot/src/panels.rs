use std::sync::Arc;

use aurorix_admin::{Panel, PanelRecord, PanelRemote, SourceError, SourceFuture};
use aurorix_api::BotApi;

#[derive(Clone)]
pub struct ApiPanelRemote {
    api: Arc<BotApi>,
}

impl std::fmt::Debug for ApiPanelRemote {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiPanelRemote")
            .finish_non_exhaustive()
    }
}

impl ApiPanelRemote {
    pub fn new(api: Arc<BotApi>) -> Self {
        Self { api }
    }

    pub fn api(&self) -> &Arc<BotApi> {
        &self.api
    }
}

fn remote_error(error: aurorix_api::ApiError) -> SourceError {
    SourceError::Server(format!("QQ 指令面板接口调用失败: {error}"))
}

fn convert<T, U>(value: &T, label: &str) -> Result<U, SourceError>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let encoded = serde_json::to_value(value)
        .map_err(|error| SourceError::Server(format!("{label}转换失败: {error}")))?;
    serde_json::from_value(encoded)
        .map_err(|error| SourceError::Server(format!("{label}转换失败: {error}")))
}

fn to_api_panel(panel: &Panel) -> Result<aurorix_api::Panel, SourceError> {
    convert(panel, "指令面板")
}

fn from_api_record(record: &aurorix_api::PanelRecord) -> Result<PanelRecord, SourceError> {
    convert(record, "指令面板")
}

impl PanelRemote for ApiPanelRemote {
    fn list(&self, scope: String) -> SourceFuture<'_, Result<Vec<PanelRecord>, SourceError>> {
        Box::pin(async move {
            let records = self.api.panel_list(&scope).await.map_err(remote_error)?;
            records.iter().map(from_api_record).collect()
        })
    }

    fn count(&self) -> SourceFuture<'_, Result<usize, SourceError>> {
        Box::pin(async move { self.api.panel_count().await.map_err(remote_error) })
    }

    fn get(&self, panel_id: String) -> SourceFuture<'_, Result<Option<PanelRecord>, SourceError>> {
        Box::pin(async move {
            let record = self.api.panel_get(&panel_id).await.map_err(remote_error)?;
            from_api_record(&record).map(Some)
        })
    }

    fn create(&self, scope: String, panel: Panel) -> SourceFuture<'_, Result<String, SourceError>> {
        Box::pin(async move {
            let api_panel = to_api_panel(&panel)?;
            self.api
                .panel_create(&scope, &api_panel)
                .await
                .map_err(remote_error)
        })
    }

    fn update(&self, panel_id: String, panel: Panel) -> SourceFuture<'_, Result<i32, SourceError>> {
        Box::pin(async move {
            let api_panel = to_api_panel(&panel)?;
            self.api
                .panel_update(&panel_id, &api_panel)
                .await
                .map_err(remote_error)
        })
    }

    fn delete(&self, panel_id: String) -> SourceFuture<'_, Result<(), SourceError>> {
        Box::pin(async move { self.api.panel_delete(&panel_id).await.map_err(remote_error) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aurorix_admin::PanelItem;

    fn panel() -> Panel {
        Panel {
            items: vec![PanelItem {
                name: String::from("签到"),
                desc: String::from("每日签到"),
                kind: String::from("command"),
                only_admin: false,
                link: String::new(),
            }],
            remark: String::from("常用指令"),
            version: 0,
        }
    }

    #[test]
    fn panel_converts_without_losing_fields() {
        let api = to_api_panel(&panel()).expect("面板要能转换");
        assert_eq!(api.items.len(), 1);
        assert_eq!(api.items[0].name, "签到");
        assert_eq!(api.items[0].kind, "command");
        assert_eq!(api.remark, "常用指令");
    }

    #[test]
    fn record_converts_back_to_admin_shape() {
        let api = aurorix_api::PanelRecord {
            panel_id: String::from("qq-9"),
            scope: String::from("group"),
            target_type: String::from("all"),
            panel: to_api_panel(&panel()).expect("面板要能转换"),
            version: 3,
            ..aurorix_api::PanelRecord::default()
        };
        let record = from_api_record(&api).expect("记录要能转换");
        assert_eq!(record.panel_id, "qq-9");
        assert_eq!(record.scope, "group");
        assert_eq!(record.panel.remark, "常用指令");
        assert_eq!(record.version, 3);
    }

    #[test]
    fn remote_errors_keep_the_platform_reason() {
        let error = remote_error(aurorix_api::ApiError::Config(String::from("令牌失效")));
        assert!(
            error.message().contains("令牌失效"),
            "错误要保留平台原因: {}",
            error.message()
        );
    }
}
