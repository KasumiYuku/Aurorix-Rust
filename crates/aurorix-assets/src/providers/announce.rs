use std::sync::Arc;

use serde::Deserialize;

use crate::error::AssetError;
use crate::http::{call, HttpClient, HttpMethod, HttpRequest, Multipart};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::bkn;

use super::util::{cfg_required, skey_from_cookie, truncate};

pub const NAME: &str = "announce";
const ENDPOINT: &str = "https://web.qun.qq.com/cgi-bin/announce/upload_img";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "QQ 群公告",
        vec![ConfigField::password("cookie", "qun.qq.com Cookie（含 skey）").required()],
        Box::new(|item, http| {
            let Some(cookie) = cfg_required(&item.config, "cookie") else {
                return Err(AssetError::message("announce: cookie 必填"));
            };
            let skey = skey_from_cookie(&cookie);
            Ok(Arc::new(AnnounceProvider { http, cookie, skey }))
        }),
    )
}

pub struct AnnounceProvider {
    http: Arc<dyn HttpClient>,
    cookie: String,
    skey: String,
}

#[derive(Debug, Default, Deserialize)]
struct AnnounceEnvelope {
    #[serde(default)]
    ec: i64,
    #[serde(default)]
    em: String,
    #[serde(default)]
    id: String,
}

#[derive(Debug, Default, Deserialize)]
struct AnnounceInner {
    #[serde(default)]
    id: String,
}

impl Provider for AnnounceProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let form = Multipart::new()
                .file(
                    "pic_up",
                    input.filename.clone(),
                    input.mime.clone(),
                    input.bytes.to_vec(),
                )
                .file(
                    "file",
                    input.filename.clone(),
                    input.mime.clone(),
                    input.bytes.to_vec(),
                )
                .field("bkn", bkn(&self.skey));
            let request = HttpRequest::new(HttpMethod::Post, ENDPOINT)
                .header("Cookie", self.cookie.clone())
                .header("Host", "web.qun.qq.com")
                .header("X-Requested-With", "XMLHttpRequest")
                .multipart(form);
            let response = call(self.http.as_ref(), request).await?;
            let raw = response.text();
            let outer: AnnounceEnvelope = serde_json::from_slice(&response.body).map_err(|_| {
                AssetError::message(format!("announce: 响应解析失败: {}", truncate(&raw, 200)))
            })?;
            if outer.ec != 0 {
                return Err(AssetError::message(format!(
                    "announce: 上传失败({}): {}",
                    outer.ec, outer.em
                )));
            }
            if outer.id.is_empty() {
                return Err(AssetError::message(format!(
                    "announce: 响应缺少 id: {}",
                    truncate(&raw, 200)
                )));
            }
            let unescaped = outer.id.replace("&quot;", "\"");
            let inner: AnnounceInner = serde_json::from_str(&unescaped).unwrap_or_default();
            if inner.id.is_empty() {
                return Err(AssetError::message(format!(
                    "announce: 无法解析图片 ID: {}",
                    truncate(&outer.id, 200)
                )));
            }
            Ok(format!("https://p.qlogo.cn/gdynamic/{}/0", inner.id))
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
        (entry().factory)(
            &ProviderItem::new(NAME).with_config("cookie", "uin=o1; skey=@abc123; p_uin=o1"),
            mock,
        )
        .expect("构造 announce 失败")
    }

    #[tokio::test]
    async fn uploads_two_file_fields_and_parses_escaped_id() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"ec":0,"em":"","id":"{&quot;id&quot;:&quot;XYZ789&quot;}"}"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://p.qlogo.cn/gdynamic/XYZ789/0");
        let request = mock.last();
        assert_eq!(request.url, ENDPOINT);
        assert_eq!(request.header_value("Host"), Some("web.qun.qq.com"));
        assert_eq!(
            request.header_value("X-Requested-With"),
            Some("XMLHttpRequest")
        );
        assert_eq!(
            request.header_value("Cookie"),
            Some("uin=o1; skey=@abc123; p_uin=o1")
        );
        let form = request.multipart_form().expect("应当是 multipart");
        assert_eq!(form.file_part("pic_up").unwrap().data, b"bytes");
        assert_eq!(form.file_part("file").unwrap().data, b"bytes");
        assert_eq!(form.field_value("bkn"), Some(bkn("@abc123").as_str()));
    }

    #[tokio::test]
    async fn error_codes_and_broken_ids_are_reported() {
        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"ec":100,"em":"没有权限"}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "announce: 上传失败(100): 没有权限");

        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"ec":0,"id":""}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert!(err.to_string().starts_with("announce: 响应缺少 id: "));

        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"ec":0,"id":"not-json"}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "announce: 无法解析图片 ID: not-json");
    }

    #[test]
    fn cookie_is_required() {
        let err = (entry().factory)(&ProviderItem::new(NAME), MockHttp::shared(vec![])).failure();
        assert_eq!(err.to_string(), "announce: cookie 必填");
    }
}
