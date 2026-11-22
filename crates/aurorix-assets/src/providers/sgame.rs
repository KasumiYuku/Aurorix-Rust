use std::sync::Arc;

use crate::error::AssetError;
use crate::http::{call, HttpClient, HttpMethod, HttpRequest};
use crate::provider::{BoxFuture, MediaKinds, Provider, UploadInput};
use crate::registry::{ConfigField, ProviderEntry};
use crate::sign::{cos_sign, md5_base64, md5_hex_upper};

use super::util::{mime_ext, mime_or, now_millis, now_unix};

pub const NAME: &str = "sgame";
const DEFAULT_HOST: &str = "sgame-data-service-1252931805.cos.ap-nanjing.myqcloud.com";
const DEFAULT_SECRET_ID: &str = "AKIDrb8TbfXAZryqTs2ygBSVJGsHTQ8dtwmN";
const DEFAULT_SECRET_KEY: &str = "PZaNxKWfcLp34rPjrutkWFyZCcyDGB0d";
const DEFAULT_BUCKET_URL: &str = "https://download.nature.qq.com";
const COS_META_EXTRA: &str = "2971978106IOS";
const HEADER_LIST: &str =
    "content-length;content-md5;content-type;host;x-cos-meta-extra;x-cos-meta-md5";

pub(crate) fn entry() -> ProviderEntry {
    ProviderEntry::new(
        NAME,
        "腾讯云 COS（sgame）",
        vec![
            ConfigField::password("secretId", "腾讯云 SecretId").description("留空用内置密钥"),
            ConfigField::password("secretKey", "腾讯云 SecretKey").description("留空用内置密钥"),
            ConfigField::text("host", "COS Host"),
            ConfigField::text("bucketUrl", "Bucket URL"),
        ],
        Box::new(|item, http| {
            Ok(Arc::new(SgameProvider {
                http,
                secret_id: super::util::cfg_str(&item.config, "secretId", DEFAULT_SECRET_ID),
                secret_key: super::util::cfg_str(&item.config, "secretKey", DEFAULT_SECRET_KEY),
                host: super::util::cfg_str(&item.config, "host", DEFAULT_HOST),
                bucket_url: super::util::cfg_str(&item.config, "bucketUrl", DEFAULT_BUCKET_URL),
            }))
        }),
    )
}

pub struct SgameProvider {
    http: Arc<dyn HttpClient>,
    secret_id: String,
    secret_key: String,
    host: String,
    bucket_url: String,
}

impl Provider for SgameProvider {
    fn name(&self) -> &str {
        NAME
    }

    fn kinds(&self) -> MediaKinds {
        MediaKinds::IMAGE | MediaKinds::VIDEO
    }

    fn max_size(&self) -> u64 {
        0
    }

    fn upload<'a>(&'a self, input: UploadInput<'a>) -> BoxFuture<'a, Result<String, AssetError>> {
        Box::pin(async move {
            let ext = mime_ext(&input.mime);
            let file_name = format!("img_{}.{ext}", now_millis());
            let url_path = format!("/SnsShare/SocialProfile/{file_name}");
            let md5_upper = md5_hex_upper(input.bytes);
            let content_md5 = md5_base64(input.bytes);
            let content_length = input.bytes.len().to_string();
            let content_type = mime_or(&input.mime);
            let now = now_unix();
            let expire = now + 600;
            let signed = vec![
                ("content-length".to_string(), content_length.clone()),
                ("content-md5".to_string(), content_md5.clone()),
                ("content-type".to_string(), content_type.clone()),
                ("host".to_string(), self.host.clone()),
                ("x-cos-meta-extra".to_string(), COS_META_EXTRA.to_string()),
                ("x-cos-meta-md5".to_string(), md5_upper.clone()),
            ];
            let (signature, q_key_time) =
                cos_sign("put", &url_path, &signed, &self.secret_key, now, expire);
            let authorization = format!(
                "q-sign-algorithm=sha1&q-ak={}&q-sign-time={now};{expire}&q-key-time={q_key_time}&q-header-list={HEADER_LIST}&q-url-param-list=&q-signature={signature}",
                self.secret_id
            );
            let request =
                HttpRequest::new(HttpMethod::Put, format!("https://{}{url_path}", self.host))
                    .header("content-length", content_length)
                    .header("content-md5", content_md5)
                    .header("content-type", content_type)
                    .header("host", self.host.clone())
                    .header("x-cos-meta-extra", COS_META_EXTRA)
                    .header("x-cos-meta-md5", md5_upper)
                    .header("Authorization", authorization)
                    .header("User-Agent", "cos-xml-ios-sdk-v6.4.0")
                    .bytes(input.bytes.to_vec());
            call(self.http.as_ref(), request).await?;
            Ok(format!("{}{url_path}", self.bucket_url))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpResponse;
    use crate::testkit::MockHttp;

    fn provider(http: Arc<MockHttp>) -> Arc<dyn Provider> {
        entry().factory.as_ref()(
            &crate::config::ProviderItem::new(NAME)
                .with_config("secretId", "AKID")
                .with_config("secretKey", "SECRET"),
            http,
        )
        .expect("构造 sgame 失败")
    }

    #[tokio::test]
    async fn uploads_with_cos_signature_headers() {
        let mock = MockHttp::shared(vec![HttpResponse::ok()]);
        let provider = provider(mock.clone());
        let url = provider
            .upload(UploadInput::new(b"\x89PNG-body", "photo.png", "image/png"))
            .await
            .expect("上传应当成功");
        assert!(url.starts_with("https://download.nature.qq.com/SnsShare/SocialProfile/img_"));
        assert!(url.ends_with(".png"));
        let request = mock.last();
        assert_eq!(request.method, HttpMethod::Put);
        assert!(request
            .url
            .starts_with("https://sgame-data-service-1252931805.cos.ap-nanjing.myqcloud.com/SnsShare/SocialProfile/img_"));
        assert_eq!(
            request.header_value("host"),
            Some("sgame-data-service-1252931805.cos.ap-nanjing.myqcloud.com")
        );
        assert_eq!(
            request.header_value("x-cos-meta-extra"),
            Some("2971978106IOS")
        );
        assert_eq!(
            request.header_value("x-cos-meta-md5"),
            Some("5F5C1FA953AECA25CBD198843F00B910")
        );
        assert_eq!(
            request.header_value("content-md5"),
            Some("X1wfqVOuyiXL0ZiEPwC5EA==")
        );
        assert_eq!(request.header_value("content-length"), Some("9"));
        assert_eq!(request.header_value("content-type"), Some("image/png"));
        assert_eq!(
            request.header_value("User-Agent"),
            Some("cos-xml-ios-sdk-v6.4.0")
        );
        let authorization = request.header_value("Authorization").unwrap();
        assert!(authorization.starts_with("q-sign-algorithm=sha1&q-ak=AKID&q-sign-time="));
        assert!(authorization.contains("q-header-list=content-length;content-md5;content-type;host;x-cos-meta-extra;x-cos-meta-md5"));
        assert!(authorization.contains("q-url-param-list=&q-signature="));
        assert_eq!(request.body_bytes(), Some(&b"\x89PNG-body"[..]));
    }

    #[tokio::test]
    async fn custom_bucket_url_is_used_for_the_final_link() {
        let mock = MockHttp::shared(vec![HttpResponse::ok()]);
        let provider = entry().factory.as_ref()(
            &crate::config::ProviderItem::new(NAME)
                .with_config("secretId", "AKID")
                .with_config("secretKey", "SECRET")
                .with_config("bucketUrl", "https://cdn.example.com"),
            mock,
        )
        .unwrap();
        let url = provider
            .upload(UploadInput::new(b"data", "a.jpg", "image/jpeg"))
            .await
            .unwrap();
        assert!(url.starts_with("https://cdn.example.com/SnsShare/SocialProfile/img_"));
        assert!(url.ends_with(".jpeg"));
    }

    #[tokio::test]
    async fn empty_config_falls_back_to_the_built_in_secrets() {
        let mock = MockHttp::shared(vec![HttpResponse::ok()]);
        let provider = entry()
            .factory
            .as_ref()(&crate::config::ProviderItem::new(NAME), mock.clone())
            .expect("空配置要能构造 sgame");
        provider
            .upload(UploadInput::new(b"data", "a.png", "image/png"))
            .await
            .expect("上传应当成功");
        let authorization = mock.last().header_value("Authorization").unwrap().to_string();
        assert!(authorization.contains(&format!("q-ak={DEFAULT_SECRET_ID}")));
    }
}
