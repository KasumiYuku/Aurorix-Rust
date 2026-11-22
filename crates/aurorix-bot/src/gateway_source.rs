use std::sync::Arc;
use std::time::Duration;

use aurorix_api::{BotApi, GatewayBotInfo};
use aurorix_gateway::{jitter, BoxFuture, GatewayEndpoint, GatewayError, GatewaySource};
use aurorix_log::Logger;

use crate::bot::MODULE;

pub const GATEWAY_INFO_FAILURE_DELAY: Duration = Duration::from_secs(5);

pub const GATEWAY_SESSION_DELAY: Duration = Duration::from_secs(3);

pub struct OfficialGatewaySource {
    api: Arc<BotApi>,
    logger: Arc<Logger>,
}

impl OfficialGatewaySource {
    pub fn new(api: Arc<BotApi>, logger: Arc<Logger>) -> Self {
        Self { api, logger }
    }

    pub fn reauth_delay_for(info: &GatewayBotInfo) -> Duration {
        if info.exhausted() {
            return info.reset_after();
        }
        jitter(GATEWAY_SESSION_DELAY)
    }
}

impl GatewaySource for OfficialGatewaySource {
    fn endpoint(&self) -> BoxFuture<'_, Result<GatewayEndpoint, GatewayError>> {
        Box::pin(async move {
            let info = self
                .api
                .gateway_bot()
                .await
                .map_err(|err| GatewayError::Transport(err.to_string()))?;
            let shards = info.shard_count();
            Ok(GatewayEndpoint::new(info.url, shards))
        })
    }

    fn reauth_delay(&self) -> BoxFuture<'_, Duration> {
        Box::pin(async move {
            let info = match self.api.gateway_bot().await {
                Ok(info) => info,
                Err(error) => {
                    self.logger.warn(
                        MODULE,
                        &format!("查询会话启动限额失败, 按默认退避重连: {error}"),
                    );
                    return jitter(GATEWAY_INFO_FAILURE_DELAY);
                }
            };
            match info.exhausted() {
                true => {
                    let delay = Self::reauth_delay_for(&info);
                    self.logger.warn(
                        MODULE,
                        &format!(
                            "会话启动配额耗尽 ({}/{}), {:.0} 分钟后重新鉴权",
                            info.session_start_limit.remaining,
                            info.session_start_limit.total,
                            delay.as_secs_f64() / 60.0
                        ),
                    );
                }
                false => self.logger.warn(
                    MODULE,
                    &format!(
                        "会话失效, 剩余启动配额 {}, 稍后重新鉴权",
                        info.session_start_limit.remaining
                    ),
                ),
            }
            Self::reauth_delay_for(&info)
        })
    }
}
