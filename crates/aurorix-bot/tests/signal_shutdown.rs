use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CHILD_ENV: &str = "AURORIX_SIGNAL_CHILD";

#[tokio::test]
#[ignore]
async fn child_awaits_shutdown_signal() {
    if std::env::var(CHILD_ENV).is_err() {
        return;
    }
    aurorix_bot::wait_for_shutdown_signal().await;
}

#[test]
fn sigterm_is_delivered_to_the_shutdown_waiter() {
    if std::env::var(CHILD_ENV).is_ok() {
        return;
    }
    let exe = std::env::current_exe().expect("定位当前测试二进制");
    let mut child = ChildGuard::spawn(
        Command::new(exe)
            .args([
                "--exact",
                "child_awaits_shutdown_signal",
                "--ignored",
                "--test-threads=1",
            ])
            .env(CHILD_ENV, "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null()),
    );

    std::thread::sleep(Duration::from_millis(800));
    let killed = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .expect("执行 kill 命令");
    assert!(killed.success(), "kill 必须成功投递 SIGTERM");

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut status = None;
    while Instant::now() < deadline {
        match child.try_wait().expect("查询子进程状态") {
            Some(finished) => {
                status = Some(finished);
                break;
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let status = status.expect("子进程必须在 10 秒内响应 SIGTERM 退出");
    assert!(
        status.success(),
        "子进程必须优雅退出而不是被信号杀死, 实际 {status:?}"
    );
}

struct ChildGuard(std::process::Child);

impl ChildGuard {
    fn spawn(command: &mut Command) -> Self {
        Self(command.spawn().expect("启动等待信号的子进程"))
    }

    fn id(&self) -> u32 {
        self.0.id()
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.0.try_wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
