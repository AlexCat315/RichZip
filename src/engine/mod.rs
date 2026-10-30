mod rar;
mod safety;
mod sevenz;
mod tar;
mod zip;

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub use safety::{ExtractionPlan, TaskWorkspace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
    Unsupported,
}

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: u64,
    pub modified: String,
    pub kind: EntryKind,
}

impl ArchiveEntry {
    pub fn file(name: String, size: u64, modified: String) -> Self {
        Self {
            name,
            size,
            modified,
            kind: EntryKind::File,
        }
    }

    pub fn directory(name: String, modified: String) -> Self {
        Self {
            name,
            size: 0,
            modified,
            kind: EntryKind::Directory,
        }
    }

    pub fn is_file(&self) -> bool {
        self.kind == EntryKind::File
    }
}

#[derive(Debug, Clone)]
pub struct ExtractionProgress {
    pub current: String,
    pub processed: u64,
    pub total: u64,
    pub completed: usize,
}

#[derive(Debug, Clone)]
pub struct ExtractReport {
    pub output_dir: PathBuf,
    pub completed: usize,
}

#[derive(Debug)]
pub struct ArchiveError {
    message: String,
    cancelled: bool,
}

impl ArchiveError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            cancelled: false,
        }
    }

    pub fn cancelled() -> Self {
        Self {
            message: "任务已取消，本次临时文件已清理".into(),
            cancelled: true,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn context(self, context: &str) -> Self {
        Self {
            message: format!("{context}：{}", self.message),
            cancelled: self.cancelled,
        }
    }
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ArchiveError {}

impl From<io::Error> for ArchiveError {
    fn from(error: io::Error) -> Self {
        Self::new(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ArchiveError>;

pub trait Archive {
    fn list(&mut self) -> Result<Vec<ArchiveEntry>>;

    fn extract(
        &mut self,
        destination: &Path,
        progress: &mut dyn FnMut(ExtractionProgress),
        cancel: &AtomicBool,
    ) -> Result<ExtractReport>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    SevenZip,
    Rar,
    Tar,
    TarGz,
}

impl ArchiveFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::SevenZip => "7Z",
            Self::Rar => "RAR",
            Self::Tar => "TAR",
            Self::TarGz => "TAR.GZ",
        }
    }
}

pub fn detect_format(path: &Path) -> Result<ArchiveFormat> {
    let lower = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if lower.ends_with(".z01")
        || lower.ends_with(".zip.001")
        || lower.ends_with(".7z.001")
        || lower.ends_with(".001")
    {
        return Err(ArchiveError::new(
            "暂不支持 ZIP/7Z 分卷，请提供完整单卷压缩包",
        ));
    }

    let mut file = File::open(path)
        .map_err(|e| ArchiveError::new(format!("无法打开 {}：{e}", path.display())))?;
    let mut header = [0u8; 512];
    let mut read = 0;
    while read < header.len() {
        let count = file.read(&mut header[read..])?;
        if count == 0 {
            break;
        }
        read += count;
    }
    let bytes = &header[..read];

    if bytes.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Ok(ArchiveFormat::SevenZip);
    }
    if bytes.starts_with(b"Rar!\x1A\x07\x00") || bytes.starts_with(b"Rar!\x1A\x07\x01\x00") {
        return Ok(ArchiveFormat::Rar);
    }
    if bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
    {
        return Ok(ArchiveFormat::Zip);
    }
    if bytes.starts_with(b"\x1F\x8B") {
        if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            return Ok(ArchiveFormat::TarGz);
        }
        return Err(ArchiveError::new(
            "单文件 GZ 将在 M2 支持；当前仅支持 tar.gz / tgz",
        ));
    }
    if bytes.len() >= 262 && &bytes[257..262] == b"ustar" || lower.ends_with(".tar") {
        return Ok(ArchiveFormat::Tar);
    }

    Err(ArchiveError::new(
        "无法识别该压缩包格式；M1 支持 zip、7z、rar、tar、tar.gz、tgz、cbz、cbr",
    ))
}

pub fn open_archive(path: &Path) -> Result<Box<dyn Archive>> {
    if !path.is_file() {
        return Err(ArchiveError::new(format!(
            "文件不存在或不是普通文件：{}",
            path.display()
        )));
    }
    match detect_format(path)? {
        ArchiveFormat::Zip => Ok(Box::new(zip::ZipBackend::new(path))),
        ArchiveFormat::SevenZip => Ok(Box::new(sevenz::SevenZipBackend::new(path))),
        ArchiveFormat::Rar => Ok(Box::new(rar::RarBackend::new(path))),
        ArchiveFormat::Tar | ArchiveFormat::TarGz => {
            Ok(Box::new(tar::TarBackend::new(path, detect_format(path)?)))
        }
    }
}

pub struct EntryProgress<'a> {
    current: String,
    processed: u64,
    total: u64,
    completed: usize,
    callback: &'a mut dyn FnMut(ExtractionProgress),
    cancel: &'a AtomicBool,
}

impl<'a> EntryProgress<'a> {
    pub fn new(
        entries: &[ArchiveEntry],
        callback: &'a mut dyn FnMut(ExtractionProgress),
        cancel: &'a AtomicBool,
    ) -> Result<Self> {
        let total =
            entries
                .iter()
                .filter(|entry| entry.is_file())
                .try_fold(0u64, |sum, entry| {
                    sum.checked_add(entry.size)
                        .ok_or_else(|| ArchiveError::new("压缩包声明的总大小超出支持范围"))
                })?;
        let mut state = Self {
            current: String::new(),
            processed: 0,
            total,
            completed: 0,
            callback,
            cancel,
        };
        state.emit();
        Ok(state)
    }

    pub fn start(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        self.current = name.to_owned();
        self.emit();
        Ok(())
    }

    pub fn copy<R: Read, W: Write>(
        &mut self,
        reader: &mut R,
        writer: &mut W,
        expected_size: u64,
    ) -> Result<()> {
        let start = self.processed;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            self.check_cancel()?;
            let count = reader
                .read(&mut buffer)
                .map_err(|e| ArchiveError::new(format!("读取 {} 失败：{e}", self.current)))?;
            if count == 0 {
                break;
            }
            writer
                .write_all(&buffer[..count])
                .map_err(|e| ArchiveError::new(format!("写入 {} 失败：{e}", self.current)))?;
            self.processed = self.processed.saturating_add(count as u64);
            self.emit();
        }
        if self.processed.saturating_sub(start) != expected_size {
            return Err(ArchiveError::new(format!(
                "{} 的内容不完整或已损坏",
                self.current
            )));
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<usize> {
        self.completed += 1;
        self.emit();
        Ok(self.completed)
    }

    pub fn completed(&self) -> usize {
        self.completed
    }

    pub fn advance(&mut self, bytes: usize) -> Result<()> {
        self.check_cancel()?;
        self.processed = self.processed.saturating_add(bytes as u64);
        self.emit();
        Ok(())
    }

    pub fn check_cancel(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(ArchiveError::cancelled())
        } else {
            Ok(())
        }
    }

    fn emit(&mut self) {
        (self.callback)(ExtractionProgress {
            current: self.current.clone(),
            processed: self.processed,
            total: self.total,
            completed: self.completed,
        });
    }
}

pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}
