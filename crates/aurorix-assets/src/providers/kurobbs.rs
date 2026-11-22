use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use crate::error::AssetError;
use crate::http::{call, HttpClient, HttpMethod, HttpRequest, Multipart};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};

use super::util::{cfg_required, mime_ext, mime_or, truncate};

pub const NAME: &str = "kurobbs";
const ENDPOINT: &str = "https://api.kurobbs.com/forum/uploadForumImgForH5";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "库街区",
        vec![ConfigField::password("token", "库街区 Token")
            .required()
            .description("库街区登录凭证，抓包任意已登录请求的请求头 Token 字段")],
        Box::new(|item, http| {
            let Some(token) = cfg_required(&item.config, "token") else {
                return Err(AssetError::message("kurobbs: token 必填"));
            };
            Ok(Arc::new(KurobbsProvider { http, token }))
        }),
    )
    .probe_skip()
}

pub struct KurobbsProvider {
    http: Arc<dyn HttpClient>,
    token: String,
}

#[derive(Debug, Deserialize)]
struct KuroResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Value,
}

impl Provider for KurobbsProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn skip_probe(&self) -> bool {
        true
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let filename = if input.filename.is_empty() {
                format!("image.{}", mime_ext(&input.mime))
            } else {
                input.filename.clone()
            };
            let form = Multipart::new().file(
                "files",
                filename,
                mime_or(&input.mime),
                input.bytes.to_vec(),
            );
            let request = HttpRequest::new(HttpMethod::Post, ENDPOINT)
                .header("User-Agent", USER_AGENT)
                .header("source", "h5")
                .header("Referer", "http://www.kurobbs.com/")
                .header("Token", self.token.clone())
                .multipart(form);
            let response = call(self.http.as_ref(), request).await?;
            let raw = response.text();
            let parsed: KuroResponse = serde_json::from_slice(&response.body).map_err(|_| {
                AssetError::message(format!("kurobbs: 返回非 JSON: {}", truncate(&raw, 200)))
            })?;
            let url = match &parsed.data {
                Value::String(text) => text.clone(),
                Value::Array(items) => items
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                _ => String::new(),
            };
            if url.is_empty() {
                return Err(AssetError::message(format!(
                    "kurobbs: 响应无图片地址(code={} {}): {}",
                    parsed.code,
                    parsed.msg,
                    truncate(&raw, 200)
                )));
            }
            Ok(url)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderItem;
    use crate::http::HttpResponse;
    use crate::testkit::MockHttp;
    use crate::testkit::ResultExt as _;

    fn provider(mock: Arc<MockHttp>) -> Arc<dyn Provider> {
        (entry().factory)(&ProviderItem::new(NAME).with_config("token", "TK"), mock)
            .expect("构造 kurobbs 失败")
    }

    #[tokio::test]
    async fn uploads_multipart_with_token_header() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"code":0,"msg":"ok","data":"https://p.kurobbs.com/a.png"}"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://p.kurobbs.com/a.png");
        let request = mock.last();
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.url, ENDPOINT);
        assert_eq!(request.header_value("Token"), Some("TK"));
        assert_eq!(request.header_value("source"), Some("h5"));
        assert_eq!(
            request.header_value("Referer"),
            Some("http://www.kurobbs.com/")
        );
        let form = request.multipart_form().expect("应当是 multipart");
        let part = form.file_part("files").expect("字段名必须是 files");
        assert_eq!(part.filename, "a.png");
        assert_eq!(part.mime, "image/png");
        assert_eq!(part.data, b"bytes");
    }

    #[tokio::test]
    async fn accepts_array_payload_and_defaults_filename() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"code":0,"data":["https://p.kurobbs.com/b.png"]}"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "", "image/jpeg"))
            .await
            .unwrap();
        assert_eq!(url, "https://p.kurobbs.com/b.png");
        let form = mock.last();
        let form = form.multipart_form().unwrap();
        assert_eq!(form.file_part("files").unwrap().filename, "image.jpeg");
    }

    #[tokio::test]
    async fn empty_payload_reports_code_and_message() {
        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"code":403,"msg":"令牌失效"}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        let message = err.to_string();
        assert!(message.starts_with("kurobbs: 响应无图片地址(code=403 令牌失效)"));
    }

    #[test]
    fn token_is_required() {
        let err = (entry().factory)(&ProviderItem::new(NAME), MockHttp::shared(vec![])).failure();
        assert_eq!(err.to_string(), "kurobbs: token 必填");
    }
}
