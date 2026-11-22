mod api;
mod scan;

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SKIP_DIRS: [&str; 3] = ["target", ".git", "node_modules"];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check-comments") => check_comments(&args[1..]),
        Some("gen-api") => gen_api(&args[1..]),
        Some("stats") => stats(&args[1..]),
        _ => {
            eprintln!("用法: xtask <check-comments | gen-api [--check] | stats> [--root <dir>]");
            ExitCode::from(2)
        }
    }
}

fn default_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn parse_root(args: &[String]) -> Result<PathBuf, String> {
    let mut idx = 0usize;
    let mut root = default_root();
    while idx < args.len() {
        match args[idx].as_str() {
            "--root" => {
                let value = args
                    .get(idx + 1)
                    .ok_or_else(|| "--root 缺少目录参数".to_string())?;
                root = PathBuf::from(value);
                idx += 2;
            }
            other => return Err(format!("未知参数: {other}")),
        }
    }
    if !root.is_dir() {
        return Err(format!("目录不存在: {}", root.display()));
    }
    Ok(root)
}

fn check_comments(args: &[String]) -> ExitCode {
    let root = match parse_root(args) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let mut files = Vec::new();
    collect_rs_files(&root, &mut files);
    files.sort();
    let mut scanned = 0usize;
    let mut total = 0usize;
    for file in &files {
        let Ok(src) = fs::read_to_string(file) else {
            eprintln!("跳过无法读取的文件: {}", file.display());
            continue;
        };
        scanned += 1;
        let shown = file.strip_prefix(&root).unwrap_or(file).display();
        for violation in scan::scan_source(&src) {
            total += 1;
            println!("{}:{}: {}", shown, violation.line, violation.snippet);
        }
    }
    println!("扫描 {} 个 .rs 文件, 违规 {} 处", scanned, total);
    if total == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            collect_rs_files(&path, out);
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn stats(args: &[String]) -> ExitCode {
    let root = match parse_root(args) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let mut crates: Vec<(String, PathBuf)> = Vec::new();
    for group in ["crates", "plugins"] {
        if let Ok(entries) = fs::read_dir(root.join(group)) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() && path.join("Cargo.toml").is_file() {
                    let name = format!("{group}/{}", entry.file_name().to_string_lossy());
                    crates.push((name, path));
                }
            }
        }
    }
    let app = root.join("src");
    if app.is_dir() {
        crates.push((String::from("aurorix (顶层入口)"), app));
    }
    let xtask = root.join("xtask");
    if xtask.join("Cargo.toml").is_file() {
        crates.push((String::from("xtask"), xtask));
    }
    let mut report: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (name, dir) in crates {
        let mut files = Vec::new();
        collect_rs_files(&dir, &mut files);
        let mut lines = 0usize;
        for file in &files {
            if let Ok(src) = fs::read_to_string(file) {
                lines += src.lines().count();
            }
        }
        report.insert(name, (files.len(), lines));
    }
    let mut total_files = 0usize;
    let mut total_lines = 0usize;
    for (name, (files, lines)) in &report {
        total_files += files;
        total_lines += lines;
        println!("{name}: {files} 个 .rs 文件, {lines} 行");
    }
    println!("合计: {total_files} 个 .rs 文件, {total_lines} 行");
    ExitCode::SUCCESS
}

fn workspace_crates(root: &Path) -> Vec<(String, PathBuf)> {
    let mut crates = Vec::new();
    for group in ["crates", "plugins"] {
        let Ok(entries) = fs::read_dir(root.join(group)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("Cargo.toml").is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                crates.push((name, path));
            }
        }
    }
    crates.sort();
    crates
}

fn gen_api(args: &[String]) -> ExitCode {
    let mut check = false;
    let mut rest: Vec<String> = Vec::new();
    for arg in args {
        if arg == "--check" {
            check = true;
        } else {
            rest.push(arg.clone());
        }
    }
    let root = match parse_root(&rest) {
        Ok(root) => root,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let crates: Vec<api::CrateApi> = workspace_crates(&root)
        .iter()
        .map(|(name, dir)| api::scan_crate(name, dir))
        .collect();
    let items: usize = crates.iter().map(api::CrateApi::item_count).sum();
    if check {
        return match api::check_all(&root, &crates) {
            Ok(()) => {
                println!(
                    "API 索引已与源码一致: {} 个 crate, {items} 条公开条目",
                    crates.len()
                );
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(1)
            }
        };
    }
    match api::write_all(&root, &crates) {
        Ok(written) => {
            println!("已写出 {written} 个文件到 docs/api, 共 {items} 条公开条目");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}
