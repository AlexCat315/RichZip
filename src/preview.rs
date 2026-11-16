//! One archive entry, materialised just far enough to look at it.
//!
//! Preview never writes beside the archive. It extracts the selected entry into
//! a private temp directory, hands the UI a path (or the text itself), and lets
//! the same selection own the cleanup. That keeps a click as cheap as a peek
//! and never leaves anything behind in the user's folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::browser::normalize_path;

/// The preview favours speed over completeness: a 4 MB text cap keeps one
/// click from decoding a novel, and the image cap stops an absurd screenshot
/// from competing with the extraction task.
const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_MEDIA_BYTES: u64 = 64 * 1024 * 1024;
const TEXT_VIEW_LIMIT: usize = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Image,
    Pdf,
    Text,
    Unsupported,
}

impl PreviewKind {
    pub fn for_extension(extension: &str) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif" => {
                Self::Image
            }
            "pdf" => Self::Pdf,
            "md" | "markdown" | "txt" | "text" | "json" | "log" | "csv" | "tsv" | "xml"
            | "toml" | "yaml" | "yml" | "ini" | "cfg" | "conf" | "rs" | "ts" | "tsx" | "js"
            | "jsx" | "py" | "rb" | "go" | "java" | "kt" | "swift" | "c" | "h" | "cpp" | "hpp"
            | "m" | "mm" | "sh" | "zsh" | "bash" | "fish" | "ps1" | "bat" | "cmd" | "html"
            | "htm" | "css" | "scss" | "less" | "sql" | "gradle" | "lock" | "gitignore" => {
                Self::Text
            }
            _ => Self::Unsupported,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "图片预览",
            Self::Pdf => "PDF 预览",
            Self::Text => "文档预览",
            Self::Unsupported => "无内嵌预览",
        }
    }
}

/// What the worker produced. Paths point into the private temp directory;
/// `directory` is the lease the UI keeps until another entry is picked.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewOutcome {
    pub kind: PreviewKind,
    pub directory: PathBuf,
    pub file: Option<PathBuf>,
    /// A PDF page rendered to a bitmap wide enough to read. The inspector and
    /// the reader draw this instead of the PDF itself, whose 612 pt page would
    /// otherwise stay at 1× no matter how large the frame is.
    pub raster: Option<PathBuf>,
    pub text: Option<String>,
    pub truncated: bool,
}

impl PreviewOutcome {
    pub fn cleanup(&self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Extract exactly one entry, without ever creating or overwriting a user file.
pub fn load(
    archive_path: &Path,
    entry_id: &str,
    kind: PreviewKind,
    generation: u64,
    cancel: &AtomicBool,
) -> Result<PreviewOutcome, String> {
    // A single click opens the inspector at once, but extraction waits out
    // the 150 ms that separate a browse from a decision. Arrow keys and
    // ⌘-click runs then invalidate their predecessor before it ever reads the
    // archive, which is what keeps the peek from flickering.
    for _ in 0..15 {
        if cancel.load(Ordering::Relaxed) {
            return Err("已取消".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("已取消".into());
    }
    let mut archive = mugenzip_core::open_archive(archive_path)
        .map_err(|error| format!("无法读取压缩包：{error}"))?;
    let entries = archive
        .list()
        .map_err(|error| format!("无法读取条目列表：{error}"))?;
    let wanted = normalize_path(entry_id);
    let entry = entries
        .iter()
        .find(|candidate| {
            candidate.is_file() && normalize_path(&candidate.name) == wanted
        })
        .ok_or_else(|| "这个条目已经不在压缩包里了".to_owned())?;

    let limit = match kind {
        PreviewKind::Text => MAX_TEXT_BYTES,
        PreviewKind::Image | PreviewKind::Pdf | PreviewKind::Unsupported => MAX_MEDIA_BYTES,
    };
    if entry.size > limit {
        return Err(format!(
            "文件超过 {}，为避免卡顿没有建立预览",
            mugenzip_core::format_size(limit)
        ));
    }

    let directory = std::env::temp_dir().join(format!(
        "mugenzip-preview-{}-{generation}",
        std::process::id()
    ));
    // A generation is unique for this process, so a stale directory is debris
    // from an interrupted run rather than live work.
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("无法创建预览临时目录：{error}"))?;

    let select = |candidate: &mugenzip_core::ArchiveEntry| {
        candidate.is_file() && normalize_path(&candidate.name) == wanted
    };
    let extracted = archive.extract_entries(
        &directory,
        &select,
        false,
        &mut |_| {},
        cancel,
    );
    if let Err(error) = extracted {
        let _ = fs::remove_dir_all(&directory);
        return Err(if error.is_cancelled() {
            "已取消".into()
        } else {
            format!("无法提取预览：{error}")
        });
    }
    if cancel.load(Ordering::Relaxed) {
        let _ = fs::remove_dir_all(&directory);
        return Err("已取消".into());
    }

    let file = directory.join(&wanted);
    if !file.is_file() {
        let _ = fs::remove_dir_all(&directory);
        return Err("预览文件没有生成".into());
    }

    let mut truncated = false;
    let text = if kind == PreviewKind::Text {
        let bytes = fs::read(&file).map_err(|error| {
            let _ = fs::remove_dir_all(&directory);
            format!("无法读取预览文件：{error}")
        })?;
        truncated = bytes.len() > TEXT_VIEW_LIMIT;
        let slice = &bytes[..bytes.len().min(TEXT_VIEW_LIMIT)];
        let content = String::from_utf8_lossy(slice).into_owned();
        Some(content)
    } else {
        None
    };
    let raster = if kind == PreviewKind::Pdf {
        rasterize_pdf(&file, &directory)
    } else {
        None
    };

    Ok(PreviewOutcome {
        kind,
        directory,
        file: Some(file),
        raster,
        text,
        truncated,
    })
}

/// A readable page bitmap. Quick Look renders the first page at the requested
/// longest edge; 1600 px puts a Letter-sized page at roughly 2×, where 12 pt
/// body copy is 24 px instead of 12.
#[cfg(target_os = "macos")]
fn rasterize_pdf(file: &Path, directory: &Path) -> Option<PathBuf> {
    const EDGE: &str = "1600";
    let status = Command::new("/usr/bin/qlmanage")
        .args(["-t", "-s", EDGE, "-o"])
        .arg(directory)
        .arg(file)
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let generated = directory.join(format!(
        "{}.png",
        file.file_name().and_then(|name| name.to_str()).unwrap_or("preview.pdf")
    ));
    if !generated.is_file() {
        return None;
    }
    Some(generated)
}

#[cfg(not(target_os = "macos"))]
fn rasterize_pdf(_file: &Path, _directory: &Path) -> Option<PathBuf> {
    None
}

/// Remove every stale preview directory owned by this process. Called when the
/// app starts and when the window closes, so a crash cannot accumulate more
/// than one session of peeks.
pub fn sweep() {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let prefix = format!("mugenzip-preview-{}-", std::process::id());
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&prefix) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
