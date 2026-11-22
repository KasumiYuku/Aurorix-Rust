use std::sync::{Arc, RwLock};

use aurorix_api::{ApiError, BoxFuture};
use aurorix_assets::registry::ProviderRegistry;
use aurorix_assets::{HostConfig, ImageHost, ImageSource, ProviderItem, UploadContext};
use aurorix_core::Target;

tokio::task_local! {
    static CURRENT_TARGET: Target;
}

pub fn current_target() -> Target {
    CURRENT_TARGET
        .try_with(|target| target.clone())
        .unwrap_or_default()
}

pub async fn with_target<F: std::future::Future>(target: Target, future: F) -> F::Output {
    CURRENT_TARGET.scope(target, future).await
}

pub struct HostImageStore {
    host: RwLock<Arc<ImageHost>>,
}

impl std::fmt::Debug for HostImageStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let host = self.host();
        formatter
            .debug_struct("HostImageStore")
            .field("providers", &host.providers())
            .field("whitelist", &host.whitelist().len())
            .finish()
    }
}

impl HostImageStore {
    pub fn new(host: Arc<ImageHost>) -> Self {
        Self {
            host: RwLock::new(host),
        }
    }

    pub fn host(&self) -> Arc<ImageHost> {
        Arc::clone(&self.host.read().expect("图床锁被毒化"))
    }

    pub fn replace(&self, host: Arc<ImageHost>) {
        *self.host.write().expect("图床锁被毒化") = host;
    }
}

impl aurorix_api::ImageStore for HostImageStore {
    fn ensure(&self, url: &str) -> BoxFuture<'_, Result<String, ApiError>> {
        let url = url.to_string();
        Box::pin(async move {
            let target = current_target();
            let context = UploadContext::new(target);
            match self.host().resolve(ImageSource::Str(&url), context).await {
                Ok(resolved) if !resolved.url.is_empty() => Ok(resolved.url),
                Ok(_) => Err(ApiError::Upload(format!("图床返回空地址: {url}"))),
                Err(error) => Err(ApiError::Upload(error.to_string())),
            }
        })
    }

    fn store(
        &self,
        bytes: &[u8],
        filename: &str,
    ) -> BoxFuture<'_, Result<aurorix_api::StoredImage, ApiError>> {
        let bytes = bytes.to_vec();
        let filename = filename.to_string();
        Box::pin(async move {
            let target = current_target();
            let context = UploadContext::new(target);
            match self
                .host()
                .resolve_bytes(&bytes, &filename, "", context)
                .await
            {
                Ok(resolved) if !resolved.url.is_empty() => Ok(aurorix_api::StoredImage {
                    url: resolved.url,
                    width: resolved.width,
                    height: resolved.height,
                }),
                Ok(_) => Err(ApiError::Upload(String::from("图床返回空地址"))),
                Err(error) => Err(ApiError::Upload(error.to_string())),
            }
        })
    }

    fn publish(
        &self,
        bytes: &[u8],
        file_type: i32,
        _filename: &str,
    ) -> BoxFuture<'_, Result<String, ApiError>> {
        let bytes = bytes.to_vec();
        Box::pin(async move {
            let Some(kind) = crate::media::media_kind(file_type) else {
                return Err(ApiError::Upload(String::from("这种媒体不进图床")));
            };
            let context = UploadContext::new(current_target());
            match self.host().resolve_media(&bytes, kind, context).await {
                Ok(resolved) if !resolved.url.is_empty() => Ok(resolved.url),
                Ok(_) => Err(ApiError::Upload(String::from("图床返回空地址"))),
                Err(error) => Err(ApiError::Upload(error.to_string())),
            }
        })
    }
}

pub async fn build_host(database: &aurorix_db::Db) -> ImageHost {
    let store = database.assets();
    let stored = store.load().await.unwrap_or_default();
    let whitelist = store.load_whitelist().await.unwrap_or_default();
    let providers = stored
        .into_iter()
        .map(|item| {
            let name = match item.name.is_empty() {
                true => item.kind.clone(),
                false => item.name.clone(),
            };
            ProviderItem {
                name,
                enabled: Some(item.enabled),
                priority: item.priority,
                probe: item.probe,
                config: item.config,
            }
        })
        .collect();
    ImageHost::new(
        HostConfig {
            providers,
            whitelist,
        },
        Arc::new(ProviderRegistry::with_builtin()),
    )
}

pub struct HostReload {
    host: Arc<HostImageStore>,
    database: Arc<aurorix_db::Db>,
}

impl HostReload {
    pub fn new(host: Arc<HostImageStore>, database: Arc<aurorix_db::Db>) -> Self {
        Self { host, database }
    }
}

impl aurorix_admin::AssetReload for HostReload {
    fn reload(&self) -> aurorix_admin::SourceFuture<'_, Result<(), aurorix_admin::SourceError>> {
        Box::pin(async move {
            let host = build_host(self.database.as_ref()).await;
            self.host.replace(Arc::new(host));
            Ok(())
        })
    }
}
