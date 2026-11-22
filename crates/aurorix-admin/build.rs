use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("缺少 CARGO_MANIFEST_DIR"));
    let dist = manifest.join("assets").join("dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let index = dist.join("index.html");
    if !index.is_file() {
        panic!(
            "管理台前端产物缺失: {} 不存在, 先执行 pnpm build 生成前端产物",
            index.display()
        );
    }
    let mut files = Vec::new();
    collect(&dist, &dist, &mut files);
    files.sort();
    let mut code = String::new();
    writeln!(
        code,
        "pub const INDEX_HTML: &str = include_str!({:?});",
        index.display().to_string()
    )
    .expect("写入清单失败");
    writeln!(
        code,
        "pub const EMBEDDED_ASSETS: [EmbeddedAsset; {}] = [",
        files.len()
    )
    .expect("写入清单失败");
    for (name, path) in &files {
        writeln!(
            code,
            "    EmbeddedAsset {{ name: {:?}, body: include_str!({:?}) }},",
            name,
            path.display().to_string()
        )
        .expect("写入清单失败");
    }
    code.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("缺少 OUT_DIR"));
    std::fs::write(out.join("dist_manifest.rs"), code).expect("写出前端清单失败");
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let name = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        if name == "index.html" {
            continue;
        }
        out.push((name, path));
    }
}
