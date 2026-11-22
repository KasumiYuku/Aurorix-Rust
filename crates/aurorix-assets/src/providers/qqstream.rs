use std::sync::Arc;

use serde::Deserialize;

use crate::error::AssetError;
use crate::http::{call, parse_json, HttpClient, HttpMethod, HttpRequest, Multipart};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::{bkn, uuid_hex};

use super::util::cfg_required;

pub const NAME: &str = "qqstream";
const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "腾讯视频",
        vec![
            ConfigField::password("cookie", "完整 Cookie（含 uin/skey/p_skey）").required(),
            ConfigField::password("skey", "QQ skey").required(),
            ConfigField::password("pSkey", "qvideo.qq.com p_skey").required(),
            ConfigField::text("uin", "QQ uin（数字QQ号）").required(),
        ],
        Box::new(|item, http| {
            let (Some(cookie), Some(skey), Some(p_skey), Some(uin)) = (
                cfg_required(&item.config, "cookie"),
                cfg_required(&item.config, "skey"),
                cfg_required(&item.config, "pSkey"),
                cfg_required(&item.config, "uin"),
            ) else {
                return Err(AssetError::message("qqstream: cookie/skey/p_skey/uin 必填"));
            };
            let _ = (p_skey, uin);
            Ok(Arc::new(QqstreamProvider { http, cookie, skey }))
        }),
    )
}

pub struct QqstreamProvider {
    http: Arc<dyn HttpClient>,
    cookie: String,
    skey: String,
}

#[derive(Debug, Default, Deserialize)]
struct QqstreamResponse {
    #[serde(default)]
    retcode: i64,
    #[serde(default)]
    result: Option<QqstreamResult>,
}

#[derive(Debug, Default, Deserialize)]
struct QqstreamResult {
    #[serde(default, rename = "fileId")]
    file_id: String,
}

impl Provider for QqstreamProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let g_tk = bkn(&self.skey);
            let form = Multipart::new()
                .field("type", "2")
                .field("detectface", "1")
                .file(
                    "image",
                    uuid_hex(),
                    input.mime.clone(),
                    input.bytes.to_vec(),
                );
            let request = HttpRequest::new(
                HttpMethod::Post,
                format!("https://qvideo.qq.com/cgi-bin/videohub/upload_image?bkn={g_tk}"),
            )
            .header("Cookie", self.cookie.clone())
            .header("Referer", "https://qvideo.qq.com/mixed/m/edit-profile.html")
            .header("User-Agent", USER_AGENT)
            .multipart(form);
            let response = call(self.http.as_ref(), request).await?;
            let parsed: QqstreamResponse = parse_json(&response)?;
            if parsed.retcode != 0 {
                return Err(AssetError::message(format!(
                    "qqstream: 上传失败: retcode={}",
                    parsed.retcode
                )));
            }
            let file_id = parsed
                .result
                .map(|result| result.file_id)
                .unwrap_or_default();
            if file_id.is_empty() {
                return Err(AssetError::message("qqstream: 上传失败"));
            }
            Ok(format!("https://p.qpic.cn/qlove_pic/0/{file_id}/340"))
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
                .with_config("cookie", "uin=o1; skey=@s")
                .with_config("skey", "@skey")
                .with_config("pSkey", "@pskey")
                .with_config("uin", "10001"),
            mock,
        )
        .expect("构造 qqstream 失败")
    }

    #[tokio::test]
    async fn upload_builds_qpic_url() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"retcode":0,"result":{"fileId":"AbCdEf"}}"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"bytes", "avatar.png", "image/png"))
            .await
            .unwrap();
        assert_eq!(url, "https://p.qpic.cn/qlove_pic/0/AbCdEf/340");
        let request = mock.last();
        assert_eq!(
            request.url,
            format!(
                "https://qvideo.qq.com/cgi-bin/videohub/upload_image?bkn={}",
                bkn("@skey")
            )
        );
        assert_eq!(
            request.header_value("Referer"),
            Some("https://qvideo.qq.com/mixed/m/edit-profile.html")
        );
        let form = request.multipart_form().expect("应当是 multipart");
        assert_eq!(form.field_value("type"), Some("2"));
        assert_eq!(form.field_value("detectface"), Some("1"));
        let part = form.file_part("image").expect("字段名必须是 image");
        assert_eq!(part.filename.len(), 32);
        assert!(part.filename.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_eq!(part.data, b"bytes");
        assert_eq!(part.mime, "image/png");
    }

    #[tokio::test]
    async fn retcode_and_missing_file_id_are_errors() {
        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"retcode":10002}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "qqstream: 上传失败: retcode=10002");

        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"{"retcode":0,"result":{"fileId":""}}"#,
        )]);
        let err = provider(mock)
            .upload(UploadInput::new(b"bytes", "a.png", "image/png"))
            .await
            .failure();
        assert_eq!(err.to_string(), "qqstream: 上传失败");
    }

    #[test]
    fn credentials_are_required() {
        let err = (entry().factory)(&ProviderItem::new(NAME), MockHttp::shared(vec![])).failure();
        assert_eq!(err.to_string(), "qqstream: cookie/skey/p_skey/uin 必填");
    }
}
