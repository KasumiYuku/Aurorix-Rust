use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::BotError;

pub const LOCK_FILE_ENV: &str = "AURORIX_LOCK_FILE";

pub const DEFAULT_LOCK_FILE: &str = "/tmp/aurorix.lock";

pub const INSTANCE_CONFLICT: &str = "已有另一个 Aurorix 实例在运行";

pub struct InstanceLock {
    path: PathBuf,
    file: File,
}

impl std::fmt::Debug for InstanceLock {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InstanceLock")
            .field("path", &self.path)
            .field("pid", &std::process::id())
            .finish()
    }
}

impl InstanceLock {
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self, BotError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|err| {
                    BotError::InstanceLock(format!(
                        "无法创建锁文件目录 {}: {err}",
                        parent.display()
                    ))
                })?;
            }
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|err| {
                BotError::InstanceLock(format!("无法打开锁文件 {}: {err}", path.display()))
            })?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let who = match holder_pid(&path) {
                    Some(pid) if process_alive(pid) => format!("pid {pid} 正在运行"),
                    Some(pid) => format!("锁文件记录的 pid {pid} 已退出"),
                    None => String::from("锁文件没有记录 pid"),
                };
                return Err(BotError::InstanceLock(format!(
                    "{INSTANCE_CONFLICT}: {} 已被占用({who})",
                    path.display()
                )));
            }
            Err(TryLockError::Error(err)) => {
                return Err(BotError::InstanceLock(format!(
                    "无法锁定 {}: {err}",
                    path.display()
                )))
            }
        }
        let _ = file
            .set_len(0)
            .and_then(|()| file.write_all(format!("{}\n", std::process::id()).as_bytes()))
            .and_then(|()| file.flush());
        Ok(Self { path, file })
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn release(&self) {
        let _ = self.file.unlock();
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        self.release();
    }
}

pub fn holder_pid(path: impl AsRef<Path>) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    text.trim().parse().ok()
}

pub fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    Path::new(&format!("/proc/{pid}")).exists()
}
