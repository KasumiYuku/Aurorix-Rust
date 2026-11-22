use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aurorix_api::{ApiError, Outbound, SendOutcome};
use aurorix_bot::{
    Bot, BotConfig, CommandRegistry, InstanceLock, Logger, MemoryStats, Options, ReplyTransport,
    StatsRecorder, TokenSource,
};
use aurorix_core::Target;
use aurorix_gateway::{BoxFuture, StaticTokenSource};

const CONFLICT: &str = "已有另一个 Aurorix 实例在运行";

struct NullTransport;

impl ReplyTransport for NullTransport {
    fn send<'a>(
        &'a self,
        target: &'a Target,
        _message: &'a Outbound,
    ) -> BoxFuture<'a, Result<SendOutcome, ApiError>> {
        let target = target.clone();
        Box::pin(async move {
            Ok(SendOutcome {
                message_id: None,
                audit_id: None,
                target,
                delivered_active: false,
                fallback: false,
            })
        })
    }
}

fn temp_lock(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aurorix-lock-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let path = dir.join("aurorix.lock");
    let _ = std::fs::remove_file(&path);
    path
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("要能探测空闲端口");
    let port = listener.local_addr().expect("本地地址").port();
    drop(listener);
    port
}

fn quiet_logger() -> Arc<Logger> {
    Logger::new(Options::default().with_console(false))
}

fn bot(config: BotConfig) -> Bot {
    Bot::builder(config)
        .with_transport(Arc::new(NullTransport) as Arc<dyn ReplyTransport>)
        .with_tokens(Arc::new(StaticTokenSource::new("token")) as Arc<dyn TokenSource>)
        .with_plugins(Arc::new(CommandRegistry::new()))
        .with_stats(Arc::new(MemoryStats::new()) as Arc<dyn StatsRecorder>)
        .with_logger(quiet_logger())
        .with_signal_handling(false)
        .build()
        .expect("装配失败")
}

#[test]
fn second_acquire_on_the_same_path_is_rejected_then_released() {
    let path = temp_lock("conflict");
    let first = InstanceLock::acquire(&path).expect("首次获取应成功");
    let error = InstanceLock::acquire(&path).expect_err("第二次必须失败");
    let text = error.to_string();
    assert!(text.contains(CONFLICT), "错误要说明原因: {text}");
    assert!(text.contains("aurorix.lock"), "错误要指出锁文件: {text}");
    drop(first);
    let third = InstanceLock::acquire(&path).expect("释放后要能重新获取");
    assert_eq!(third.path(), path.as_path());
}

#[test]
fn different_paths_coexist() {
    let left = temp_lock("left");
    let right = temp_lock("right");
    let first = InstanceLock::acquire(&left).expect("左锁要能获取");
    let second = InstanceLock::acquire(&right).expect("右锁要能获取");
    assert_ne!(first.path(), second.path(), "两条路径互不影响");
}

#[test]
fn stale_pid_file_is_reclaimed() {
    let path = temp_lock("stale");
    std::fs::write(&path, format!("{}\n", u32::MAX)).expect("写入陈旧 PID");
    assert!(
        !aurorix_bot::process_alive(u32::MAX),
        "u32::MAX 不可能是活着的 pid"
    );
    let lock = InstanceLock::acquire(&path).expect("死 PID 的锁文件要被回收");
    let text = std::fs::read_to_string(&path).expect("读锁文件");
    assert!(
        text.contains(&std::process::id().to_string()),
        "回收后要写上当前进程 pid: {text}"
    );
    drop(lock);
}

#[tokio::test]
async fn bot_start_is_rejected_while_another_instance_holds_the_lock() {
    let path = temp_lock("bot");
    let mut config = BotConfig::new("appid-lock", "secret-lock")
        .with_intents(vec![String::from("GROUP_AT_MESSAGE_CREATE")])
        .with_admin("127.0.0.1", free_port())
        .with_lock_file(path.to_string_lossy().to_string());
    config.protocol = String::from("webhook");
    let first = bot(config.clone());
    let handle = first.start().await.expect("首次启动应成功");
    let second = bot(config);
    let error = second.start().await.expect_err("第二个实例必须被拒绝");
    let text = error.to_string();
    assert!(text.contains(CONFLICT), "错误要说明冲突: {text}");
    assert_eq!(second.live_tasks(), 0, "拿不到锁不该留下后台任务");
    let report = first
        .shutdown(Duration::from_secs(3))
        .await
        .expect("停机失败");
    assert_eq!(report.live_tasks, 0, "停机后不许留下后台任务");
    drop(handle);
    let again = InstanceLock::acquire(&path).expect("停机后锁应释放");
    drop(again);
}
