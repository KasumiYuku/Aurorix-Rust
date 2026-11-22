use std::sync::Arc;

use crate::error::AssetError;
use crate::http::{call, HttpClient, HttpMethod, HttpRequest};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::{base64_encode, bkn};

use super::util::{cfg_required, regex_origin_url, truncate};

pub const NAME: &str = "qzone";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "QQ 空间",
        vec![
            ConfigField::password("cookie", "qzone.qq.com Cookie").required(),
            ConfigField::password("pSkey", "QQ p_skey").required(),
            ConfigField::password("skey", "QQ skey").required(),
            ConfigField::text("pUin", "QQ p_uin（数字QQ号）").required(),
        ],
        Box::new(|item, http| {
            let (Some(cookie), Some(p_skey), Some(skey), Some(p_uin)) = (
                cfg_required(&item.config, "cookie"),
                cfg_required(&item.config, "pSkey"),
                cfg_required(&item.config, "skey"),
                cfg_required(&item.config, "pUin"),
            ) else {
                return Err(AssetError::message("qzone: cookie/p_skey/skey/p_uin 必填"));
            };
            Ok(Arc::new(QzoneProvider {
                http,
                cookie,
                p_skey,
                skey,
                p_uin,
            }))
        }),
    )
}

pub struct QzoneProvider {
    http: Arc<dyn HttpClient>,
    cookie: String,
    p_skey: String,
    skey: String,
    p_uin: String,
}

impl QzoneProvider {
    fn form(&self, data: &[u8]) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = vec![
            ("filename", "filename".to_string()),
            ("uin", self.p_uin.clone()),
            ("skey", self.skey.clone()),
            ("zzpaneluin", self.p_uin.clone()),
            ("zzpanelkey", String::new()),
            ("p_uin", self.p_uin.clone()),
            ("p_skey", self.p_skey.clone()),
            ("qzonetoken", String::new()),
            ("uploadtype", "1".to_string()),
            ("albumtype", "7".to_string()),
            ("exttype", "0".to_string()),
            ("refer", "shuoshuo".to_string()),
            ("output_type", "jsonhtml".to_string()),
            ("charset", "utf-8".to_string()),
            ("output_charset", "utf-8".to_string()),
            ("upload_hd", "1".to_string()),
            ("hd_width", "2048".to_string()),
            ("hd_height", "10000".to_string()),
            ("hd_quality", "96".to_string()),
            ("backUrls", String::new()),
            ("url", String::new()),
            ("base64", "1".to_string()),
            ("jsonhtml_callback", "callback".to_string()),
            ("picfile", base64_encode(data)),
            (
                "qzreferrer",
                format!(
                    "https%3A%2F%2Fuser.qzone.qq.com%2F{}%2Finfocenter",
                    self.p_uin
                ),
            ),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs
    }
}

impl Provider for QzoneProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let g_tk = bkn(&self.p_skey);
            let url =
                format!("https://up.qzone.qq.com/cgi-bin/upload/cgi_upload_image?g_tk={g_tk}");
            let request = HttpRequest::new(HttpMethod::Post, url)
                .header("Cookie", self.cookie.clone())
                .header("Referer", "https://user.qzone.qq.com/")
                .header("User-Agent", USER_AGENT)
                .form(self.form(input.bytes));
            let response = call(self.http.as_ref(), request).await?;
            let raw = response.text();
            let found = regex_origin_url(&response.body);
            if found.is_empty() {
                return Err(AssetError::message(format!(
                    "qzone: 上传失败: {}",
                    truncate(&raw, 200)
                )));
            }
            Ok(found)
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
                .with_config("cookie", "skey=@abc")
                .with_config("pSkey", "@pskey")
                .with_config("skey", "@skey")
                .with_config("pUin", "10001"),
            mock,
        )
        .expect("构造 qzone 失败")
    }

    #[tokio::test]
    async fn posts_form_with_base64_payload_and_bkn() {
        let mock = MockHttp::shared(vec![HttpResponse::json(
            r#"callback({"origin_url":"https:\/\/a.qpic.cn\/b.jpg"})"#,
        )]);
        let url = provider(mock.clone())
            .upload(UploadInput::new(b"jpeg", "a.jpg", "image/jpeg"))
            .await
            .unwrap();
        assert_eq!(url, "https://a.qpic.cn/b.jpg");
        let request = mock.last();
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(
            request.url,
            format!(
                "https://up.qzone.qq.com/cgi-bin/upload/cgi_upload_image?g_tk={}",
                bkn("@pskey")
            )
        );
        assert_eq!(request.header_value("Cookie"), Some("skey=@abc"));
        assert_eq!(
            request.header_value("Referer"),
            Some("https://user.qzone.qq.com/")
        );
        assert_eq!(request.form_value("picfile"), Some("anBlZw=="));
        assert_eq!(request.form_value("base64"), Some("1"));
        assert_eq!(request.form_value("filename"), Some("filename"));
        assert_eq!(request.form_value("uin"), Some("10001"));
        assert_eq!(request.form_value("p_uin"), Some("10001"));
        assert_eq!(
            request.form_value("qzreferrer"),
            Some("https%3A%2F%2Fuser.qzone.qq.com%2F10001%2Finfocenter")
        );
    }

    #[tokio::test]
    async fn missing_origin_url_reports_upload_failure() {
        let mock = MockHttp::shared(vec![HttpResponse::json(r#"{"code":-1}"#)]);
        let err = provider(mock)
            .upload(UploadInput::new(b"jpeg", "a.jpg", "image/jpeg"))
            .await
            .failure();
        assert!(err.to_string().starts_with("qzone: 上传失败: "));
    }

    #[test]
    fn all_four_fields_are_required() {
        let mock = MockHttp::shared(vec![]);
        let err =
            (entry().factory)(&ProviderItem::new(NAME).with_config("cookie", "c"), mock).failure();
        assert_eq!(err.to_string(), "qzone: cookie/p_skey/skey/p_uin 必填");
    }
}
