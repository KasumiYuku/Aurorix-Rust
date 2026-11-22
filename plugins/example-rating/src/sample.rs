use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

pub const SAMPLE_ENV: &str = "AURORIX_SAMPLE_OUT";

pub const SAMPLE_PNG_NAME: &str = "sample-b30-card.png";

pub const SAMPLE_WEBP_NAME: &str = "sample-b30-card.webp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampleOutput {
    pub png: PathBuf,
    pub webp: PathBuf,
}

pub fn sample_output_dir_from(value: Option<&OsStr>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

pub fn sample_output_dir() -> Option<PathBuf> {
    sample_output_dir_from(std::env::var_os(SAMPLE_ENV).as_deref())
}

pub fn sample_output_paths(dir: &Path) -> (PathBuf, PathBuf) {
    (dir.join(SAMPLE_PNG_NAME), dir.join(SAMPLE_WEBP_NAME))
}

pub fn emit_sample_outputs(
    dir: Option<&Path>,
    png: &[u8],
    webp: &[u8],
) -> io::Result<Option<SampleOutput>> {
    let Some(dir) = dir else {
        return Ok(None);
    };
    std::fs::create_dir_all(dir)?;
    let (png_path, webp_path) = sample_output_paths(dir);
    std::fs::write(&png_path, png)?;
    std::fs::write(&webp_path, webp)?;
    Ok(Some(SampleOutput {
        png: png_path,
        webp: webp_path,
    }))
}
