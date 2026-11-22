use std::sync::Arc;

use serde::Deserialize;

use crate::error::AssetError;
use crate::http::{call, parse_json, HttpClient, HttpMethod, HttpRequest, Multipart};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};

use super::util::cfg_required;

pub const NAME: &str = "bilibili";
const ENDPOINT: &str = "https://api.bilibili.com/x/upload/web/image";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "B 站相册",
        vec![
            ConfigField::password("sessdata", "SESSDATA").required(),
            ConfigField::password("biliJct", "bili_jct").required(),
        ],
        Box::new(|item, http| {
            let Some(sessdata) = cfg_required(&item.config, "sessdata") else {
                return Err(AssetError::message("bilibili: SESSDATA 与 bili_jct 必填"));
            };
            let Some(bili_jct) = cfg_required(&item.config, "biliJct") else {
                return Err(AssetError::message("bilibili: SESSDATA 与 bili_jct 必填"));
            };
            Ok(Arc::new(BilibiliProvider {
                http,
                sessdata,
                bili_jct,
            }))
        }),
    )
}

pub struct BilibiliProvider {
    http: Arc<dyn HttpClient>,
    sessdata: String,
    bili_jct: String,
}

#[derive(Debug, Default, Deserialize)]
struct BiliResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: BiliData,
}

#[derive(Debug, Default, Deserialize)]
struct BiliData {
    #[serde(default)]
    location: String,
}

impl Provider for BilibiliProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let form = Multipart::new()
                .field("bucket", "album")
                .field("csrf", self.bili_jct.clone())
                .file(
                    "file",
                    input.filename.clone(),
                    input.mime.clone(),
                    input.bytes.to_vec(),
                );
            let request = HttpRequest::new(HttpMethod::Post, ENDPOINT)
                .header(
                    "Cookie",
                    format!("SESSDATA={}; bili_jct={}", self.sessdata, self.bili_jct),
                )
                .header("User-Agent", USER_AGENT)
                .multipart(form);
            let response = call(self.http.as_ref(), request).await?;
            let parsed: BiliResponse = parse_json(&response)?;
            if parsed.code != 0 || parsed.data.location.is_empty() {
                return Err(AssetError::message(format!("bilibili: {}", parsed.message)));
            }
            Ok(parsed.data.location.replacen("http://", "https://", 1))
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
            &ProviderItem::new(NAME)
                .with_config("sessdata", "SESS")
                .with_config("biliJct", "JCT"),
            mock,
        )
        .expect("构造 bilibili 失败")
    }

    #[tokio::test]
    async fn uploads_to_album_bucket_and_upgrades_scheme() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"code":0,"message":"ok","data":{"location":"http://i0.hdslb.com/bfs/new_dyn/a.png"}}"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://i0.hdslb.com/bfs/new_dyn/a.png");
        let request = mock.last();
        assert_eq!(request.url, ENDPOINT);
        assert_eq!(
            request.header_value("Cookie"),
            Some("SESSDATA=SESS; bili_jct=JCT")
        );
        let form = request.multipart_form().expect("应当是 multipart");
        assert_eq!(form.field_value("bucket"), Some("album"));
        assert_eq!(form.field_value("csrf"), Some("JCT"));
        assert_eq!(form.file_part("file").unwrap().data, b"bytes");
    }

    #[tokio::test]
    async fn business_error_is_reported_with_message() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"code":-101,"message":"账号未登录","data":{"location":""}}"#,
        )]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "bilibili: 账号未登录");
    }

    #[test]
    fn credentials_are_required() {
        let err = (entry().factory)(&ProviderItem::new(NAME), MockHttp::shared(vec![])).failure();
        assert_eq!(err.to_string(), "bilibili: SESSDATA 与 bili_jct 必填");
    }
}
