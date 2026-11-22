use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use crate::error::AssetError;
use crate::http::{call, parse_json, HttpClient, HttpMethod, HttpRequest};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::uuid_hex;

use super::util::{cfg_int, cfg_required, cfg_str, merge_query, mime_ext, wrap};

pub const NAME: &str = "cnb";
const DEFAULT_BASE_URL: &str = "https://api.cnb.cool";
const CNB_SITE: &str = "https://cnb.cool";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "CNB 仓库",
        vec![
            ConfigField::password("token", "CNB 令牌")
                .required()
                .description("CNB 个人访问令牌 (Personal Access Token)，需带仓库读写权限"),
            ConfigField::text("defaultRepo", "默认仓库")
                .required()
                .placeholder("用户名/仓库名"),
            ConfigField::text("baseUrl", "API 地址").default_value(DEFAULT_BASE_URL),
            ConfigField::number("autodelete", "自动删除(秒)")
                .default_value(30)
                .description("上传成功 N 秒后删除远端文件，0 表示不删除"),
        ],
        Box::new(|item, http| {
            let Some(token) = cfg_required(&item.config, "token") else {
                return Err(AssetError::message("cnb: token 必填"));
            };
            let Some(repo) = cfg_required(&item.config, "defaultRepo") else {
                return Err(AssetError::message("cnb: 默认仓库必填"));
            };
            Ok(Arc::new(CnbProvider {
                http,
                token,
                repo,
                base_url: cfg_str(&item.config, "baseUrl", DEFAULT_BASE_URL),
                autodelete: cfg_int(&item.config, "autodelete", 30),
            }))
        }),
    )
    .probe_skip()
}

pub struct CnbProvider {
    http: Arc<dyn HttpClient>,
    token: String,
    repo: String,
    base_url: String,
    autodelete: i64,
}

#[derive(Debug, Default, Deserialize)]
struct CnbUploadInfo {
    #[serde(default)]
    upload_url: String,
    #[serde(default)]
    token: String,
    #[serde(default)]
    form: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    assets: Option<CnbAsset>,
}

#[derive(Debug, Default, Deserialize)]
struct CnbAsset {
    #[serde(default)]
    url: String,
    #[serde(default)]
    path: String,
}

impl Provider for CnbProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE | MediaKinds::VIDEO
    }

    fn max_size(&self) -> u64 {
        0
    }

    fn skip_probe(&self) -> bool {
        true
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let name = if input.filename.is_empty() {
                format!("{}.{}", uuid_hex(), mime_ext(&input.mime))
            } else {
                input.filename.clone()
            };
            let kind = if is_image_name(&name) {
                "imgs"
            } else {
                "files"
            };
            let endpoint = format!("{}/{}/-/upload/{}", self.base_url, self.repo, kind);
            let request = HttpRequest::new(HttpMethod::Post, endpoint)
                .header("accept", "application/json")
                .header("Authorization", format!("Bearer {}", self.token))
                .json(&json!({"name": name, "size": input.bytes.len()}))
                .map_err(|err| wrap("cnb: 获取上传地址失败", err))?;
            let response = call(self.http.as_ref(), request)
                .await
                .map_err(|err| wrap("cnb: 获取上传地址失败", err))?;
            let info: CnbUploadInfo =
                parse_json(&response).map_err(|err| wrap("cnb: 获取上传地址失败", err))?;
            if info.upload_url.is_empty() {
                return Err(AssetError::message("cnb: 未返回上传地址"));
            }
            let upload_url = merge_query(&info.upload_url, &info.form);
            let mut put = HttpRequest::new(HttpMethod::Put, upload_url)
                .header("Content-Type", "application/octet-stream")
                .bytes(input.bytes.to_vec());
            if !info.token.is_empty() {
                put = put.header("Authorization", format!("Bearer {}", info.token));
            }
            call(self.http.as_ref(), put)
                .await
                .map_err(|err| wrap("cnb: 上传失败", err))?;

            let asset = info.assets.unwrap_or_default();
            let mut asset_url = asset.url.clone();
            if asset_url.is_empty() {
                asset_url = asset.path.clone();
            }
            if asset_url.is_empty() {
                return Err(AssetError::message("cnb: 返回 URL 为空"));
            }
            if !asset_url.starts_with("http") {
                asset_url = format!("{CNB_SITE}{asset_url}");
            }
            if self.autodelete > 0 && !asset.path.is_empty() {
                let http = Arc::clone(&self.http);
                let token = self.token.clone();
                let base_url = self.base_url.clone();
                let path = asset.path;
                let seconds = self.autodelete as u64;
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(seconds)).await;
                    let _ = delete_file(http, base_url, token, path).await;
                });
            }
            Ok(asset_url)
        })
    }
}

async fn delete_file(
    http: Arc<dyn HttpClient>,
    base_url: String,
    token: String,
    path: String,
) -> Result<(), AssetError> {
    let url = if path.starts_with("http") {
        path
    } else {
        format!("{}/{}", base_url, path.trim_start_matches('/'))
    };
    let request = HttpRequest::new(HttpMethod::Delete, url)
        .header("Authorization", format!("Bearer {token}"));
    match call(http.as_ref(), request).await {
        Ok(_) => Ok(()),
        Err(AssetError::Status { status: 404, .. }) => Ok(()),
        Err(err) => Err(err),
    }
}

fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext = match lower.rsplit_once('.') {
        Some((_, tail)) => tail.to_string(),
        None => String::new(),
    };
    matches!(
        ext.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderItem;
    use crate::http::HttpResponse;
    use crate::testkit::MockHttp;
    use crate::testkit::ResultExt as _;

    fn provider(http: Arc<MockHttp>, extra: ProviderItem) -> Arc<dyn Provider> {
        let item = ProviderItem::new(NAME)
            .with_config("token", "PAT")
            .with_config("defaultRepo", "user/repo");
        let mut merged = item;
        for (key, value) in extra.config {
            merged.config.insert(key, value);
        }
        (entry().factory)(&merged, http).expect("构造 cnb 失败")
    }

    #[tokio::test]
    async fn three_step_upload_builds_public_url() {
        let mock = MockHttp::shared(vec![
            HttpResponse::json(
                r#"{"upload_url":"https://upload.cnb.cool/u/1","token":"UP","assets":{"url":"","path":"/assets/abc.png"}}"#,
            ),
            HttpResponse::ok(),
        ]);
        let provider = provider(
            mock.clone(),
            ProviderItem::new(NAME).with_config("autodelete", 0),
        );
        let url = provider
            .upload(UploadInput::new(b"png-bytes", "abc.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://cnb.cool/assets/abc.png");
        assert_eq!(mock.count(), 2);

        let prepare = mock.request(0);
        assert_eq!(prepare.method, HttpMethod::Post);
        assert_eq!(prepare.url, "https://api.cnb.cool/user/repo/-/upload/imgs");
        assert_eq!(prepare.header_value("Authorization"), Some("Bearer PAT"));
        assert_eq!(prepare.header_value("accept"), Some("application/json"));
        assert_eq!(
            prepare
                .body_bytes()
                .map(|raw| String::from_utf8_lossy(raw).to_string()),
            Some(r#"{"name":"abc.png","size":9}"#.to_string())
        );

        let put = mock.request(1);
        assert_eq!(put.method, HttpMethod::Put);
        assert_eq!(put.url, "https://upload.cnb.cool/u/1");
        assert_eq!(
            put.header_value("Content-Type"),
            Some("application/octet-stream")
        );
        assert_eq!(put.header_value("Authorization"), Some("Bearer UP"));
    }

    #[tokio::test]
    async fn form_fields_are_appended_to_the_upload_url() {
        let mock = MockHttp::shared(vec![
            HttpResponse::json(
                r#"{"upload_url":"https://upload.cnb.cool/u/2?b=9","form":{"a":"1","b":"2"},"assets":{"url":"https://cdn.cnb.cool/x.zip"}}"#,
            ),
            HttpResponse::ok(),
        ]);
        let provider = provider(
            mock.clone(),
            ProviderItem::new(NAME).with_config("autodelete", 0),
        );
        let url = provider
            .upload(UploadInput::new(b"zip", "x.zip", "application/zip"))
            .await
            .unwrap();
        assert_eq!(url, "https://cdn.cnb.cool/x.zip");
        assert_eq!(
            mock.request(0).url,
            "https://api.cnb.cool/user/repo/-/upload/files"
        );
        assert_eq!(mock.request(1).url, "https://upload.cnb.cool/u/2?a=1&b=2");
    }

    #[tokio::test]
    async fn missing_asset_url_is_an_error() {
        let mock = MockHttp::shared(vec![
            HttpResponse::json(r#"{"upload_url":"https://upload.cnb.cool/u/3"}"#),
            HttpResponse::ok(),
        ]);
        let provider = provider(mock, ProviderItem::new(NAME).with_config("autodelete", 0));
        let err = provider
            .upload(UploadInput::new(b"data", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "cnb: 返回 URL 为空");
    }

    #[test]
    fn required_config_is_enforced() {
        let mock = MockHttp::shared(vec![]);
        let err = (entry().factory)(&ProviderItem::new(NAME), mock.clone()).failure();
        assert_eq!(err.to_string(), "cnb: token 必填");
        let err =
            (entry().factory)(&ProviderItem::new(NAME).with_config("token", "t"), mock).failure();
        assert_eq!(err.to_string(), "cnb: 默认仓库必填");
    }

    #[test]
    fn image_extensions_choose_the_imgs_bucket() {
        assert!(is_image_name("a.PNG"));
        assert!(is_image_name("dir/a.webp"));
        assert!(!is_image_name("a.mp4"));
        assert!(!is_image_name("noext"));
    }
}
