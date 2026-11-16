//! Writing archives: the other half of the product.
//!
//! Extraction has to cope with whatever the world produces; creation only has
//! to produce something readable. That asymmetry is the reason this module is
//! so much smaller: three formats, one entry plan, one temp file.
//!
//! The flow is deliberately the same as extraction's, so the UI can treat both
//! as the same kind of job:
//!
//! 1. plan the entries (walk the sources once, so progress can report a total),
//! 2. write into a `.part` file next to the destination,
//! 3. rename it into place, on a name that is not already taken.
//!
//! Cancelling removes the temp file, so a cancelled run never leaves a half
//! written archive behind.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use crate::{ArchiveError, Result, archive_base_name};

/// What the user is allowed to write. RAR is missing on purpose: `unrar` reads,
/// it does not write, and no license here permits shipping a RAR encoder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreateFormat {
    Zip,
    TarGz,
    SevenZ,
}

impl CreateFormat {
    pub const ALL: [Self; 3] = [Self::Zip, Self::TarGz, Self::SevenZ];

    /// File extension, without the dot.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::SevenZ => "7z",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::TarGz => "TAR.GZ",
            Self::SevenZ => "7Z",
        }
    }

    /// What this format is actually good at, in the order a person picks it.
    pub const fn note(self) -> &'static str {
        match self {
            Self::Zip => "哪里都能打开",
            Self::TarGz => "保留权限与符号链接",
            Self::SevenZ => "体积最小",
        }
    }
}

/// Compression effort, not a number: the numbers differ per format and per
/// library, and what a person chooses between is "快" and "小".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressionLevel {
    Store,
    Fast,
    Balanced,
    Maximum,
}

impl CompressionLevel {
    pub const ALL: [Self; 4] = [Self::Store, Self::Fast, Self::Balanced, Self::Maximum];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Store => "仅打包",
            Self::Fast => "快速",
            Self::Balanced => "标准",
            Self::Maximum => "最大",
        }
    }

    /// The 0-9 scale deflate and gzip share.
    const fn deflate(self) -> u32 {
        match self {
            Self::Store => 0,
            Self::Fast => 1,
            Self::Balanced => 6,
            Self::Maximum => 9,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CreateRequest {
    /// Files and folders to pack. Their own names become the archive's roots.
    pub sources: Vec<PathBuf>,
    /// Folder the finished archive lands in.
    pub destination_dir: PathBuf,
    /// Archive name, with or without the extension: the format owns it.
    pub name: String,
    pub format: CreateFormat,
    pub level: CompressionLevel,
}

#[derive(Clone, Debug)]
pub struct CreateProgress {
    /// Entry being written, relative to the archive root.
    pub current: String,
    pub processed: u64,
    pub total: u64,
    pub index: usize,
    pub count: usize,
}

#[derive(Clone, Debug)]
pub struct CreateReport {
    pub path: PathBuf,
    pub entries: usize,
    /// Entries left out because they were symlinks or special files.
    pub skipped: usize,
    pub bytes: u64,
}

/// Packs `request` into a new archive. Runs on the calling thread: the UI puts
/// it on a worker, the way extraction does.
pub fn create_archive(
    request: &CreateRequest,
    progress: &mut dyn FnMut(&CreateProgress),
    cancel: &AtomicBool,
) -> Result<CreateReport> {
    if request.sources.is_empty() {
        return Err(ArchiveError::new("没有要压缩的内容"));
    }
    fs::create_dir_all(&request.destination_dir)?;
    let destination = unique_archive_path(&request.destination_dir, &request.name, request.format);

    let temp = destination.with_file_name(format!(
        ".{}.{}.part",
        destination
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        std::process::id()
    ));
    let planned = plan_entries(&request.sources, &temp)?;
    if planned.entries.is_empty() {
        return Err(ArchiveError::new("这些内容里没有可以打包的文件"));
    }

    let outcome = match request.format {
        CreateFormat::Zip => write_zip(&planned, &temp, request.level, progress, cancel),
        CreateFormat::TarGz => write_tar_gz(&planned, &temp, request.level, progress, cancel),
        CreateFormat::SevenZ => write_seven_zip(&planned, &temp, request.level, progress, cancel),
    };
    match outcome {
        Ok(stats) => {
            if let Err(error) = fs::rename(&temp, &destination) {
                let _ = fs::remove_file(&temp);
                return Err(ArchiveError::new(format!("无法写入压缩包：{error}")));
            }
            Ok(CreateReport {
                path: destination,
                entries: planned.entries.len(),
                skipped: planned.skipped,
                bytes: stats,
            })
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// `name.zip`, or the first `name (n).zip` that is free. The extension is
/// chosen by the format, so a name typed as `报告.zip` still works when the
/// user switches to 7Z.
pub fn unique_archive_path(directory: &Path, name: &str, format: CreateFormat) -> PathBuf {
    let stem = archive_base_name(Path::new(name));
    let base = directory.join(format!("{stem}.{}", format.extension()));
    if fs::symlink_metadata(&base).is_err() {
        return base;
    }
    for index in 2..1000 {
        let candidate = directory.join(format!("{stem} ({index}).{}", format.extension()));
        if fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }
    base
}

struct PlannedEntry {
    path: PathBuf,
    /// Archive-internal name, always with `/` separators.
    name: String,
    is_dir: bool,
    bytes: u64,
    modified: Option<u64>,
}

struct Plan {
    entries: Vec<PlannedEntry>,
    skipped: usize,
    total: u64,
}

/// Walks the sources once. Symlinks and special files are skipped rather than
/// followed: following them can escape the tree the user selected, and a zip
/// has no way to store a link anyway.
fn plan_entries(sources: &[PathBuf], temp: &Path) -> Result<Plan> {
    let mut plan = Plan {
        entries: Vec::new(),
        skipped: 0,
        total: 0,
    };
    for source in sources {
        let metadata = fs::symlink_metadata(source).map_err(|error| {
            ArchiveError::new(format!("无法读取 {}：{error}", source.display()))
        })?;
        let Some(name) = source.file_name().map(|name| name.to_string_lossy().into_owned()) else {
            return Err(ArchiveError::new(format!(
                "不是可打包的路径：{}",
                source.display()
            )));
        };
        if metadata.file_type().is_symlink() {
            plan.skipped += 1;
            continue;
        }
        collect(&mut plan, source, &name, &metadata, temp)?;
    }
    Ok(plan)
}

fn collect(
    plan: &mut Plan,
    path: &Path,
    name: &str,
    metadata: &fs::Metadata,
    temp: &Path,
) -> Result<()> {
    if metadata.is_dir() {
        // An archive written into the folder it packs would try to include
        // itself. The temp file is filtered by name at the file level.
        plan.entries.push(PlannedEntry {
            path: path.to_path_buf(),
            name: name.to_owned(),
            is_dir: true,
            bytes: 0,
            modified: modified_seconds(metadata),
        });
        let mut children = BTreeMap::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let child = entry.path();
            let child_meta = fs::symlink_metadata(&child)?;
            if child_meta.file_type().is_symlink() {
                plan.skipped += 1;
                continue;
            }
            if !child_meta.is_dir() && !child_meta.is_file() {
                plan.skipped += 1;
                continue;
            }
            children.insert(entry.file_name().to_string_lossy().into_owned(), child);
        }
        // Sorted so two runs of the same input produce the same archive.
        for (child_name, child) in children {
            let meta = fs::symlink_metadata(&child)?;
            let child_name = format!("{name}/{child_name}");
            collect(plan, &child, &child_name, &meta, temp)?;
        }
    } else if metadata.is_file() {
        // The temp file lives beside the sources when the user packs a folder
        // into itself; never let the archive swallow its own output.
        if path == temp {
            plan.skipped += 1;
            return Ok(());
        }
        plan.entries.push(PlannedEntry {
            path: path.to_path_buf(),
            name: name.to_owned(),
            is_dir: false,
            bytes: metadata.len(),
            modified: modified_seconds(metadata),
        });
        plan.total += metadata.len();
    }
    Ok(())
}

fn modified_seconds(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
}

fn cancelled_error() -> ArchiveError {
    ArchiveError::cancelled()
}

fn write_zip(
    plan: &Plan,
    destination: &Path,
    level: CompressionLevel,
    progress: &mut dyn FnMut(&CreateProgress),
    cancel: &AtomicBool,
) -> Result<u64> {
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    let file = File::create(destination)?;
    let mut writer = ZipWriter::new(BufWriter::new(file));
    let method = if level == CompressionLevel::Store {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflated
    };
    let directory_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    let file_options = SimpleFileOptions::default()
        .compression_method(method)
        .unix_permissions(0o644)
        .large_file(true);

    let mut processed = 0u64;
    for (index, entry) in plan.entries.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }
        progress(&CreateProgress {
            current: entry.name.clone(),
            processed,
            total: plan.total,
            index,
            count: plan.entries.len(),
        });
        if entry.is_dir {
            writer
                .add_directory(format!("{}/", entry.name), directory_options)
                .map_err(|error| ArchiveError::new(format!("写入 {} 失败：{error}", entry.name)))?;
            continue;
        }
        let file = File::open(&entry.path)
            .map_err(|error| ArchiveError::new(format!("读取 {} 失败：{error}", entry.path.display())))?;
        writer
            .start_file(entry.name.clone(), file_options)
            .map_err(|error| ArchiveError::new(format!("写入 {} 失败：{error}", entry.name)))?;
        let mut reader = file;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(cancelled_error());
            }
            let read = io::Read::read(&mut reader, &mut buffer)?;
            if read == 0 {
                break;
            }
            io::Write::write_all(&mut writer, &buffer[..read])?;
            processed += read as u64;
            progress(&CreateProgress {
                current: entry.name.clone(),
                processed,
                total: plan.total,
                index,
                count: plan.entries.len(),
            });
        }
    }
    let mut buffer = writer
        .finish()
        .map_err(|error| ArchiveError::new(format!("收尾失败：{error}")))?;
    io::Write::flush(&mut buffer)?;
    Ok(processed)
}

fn write_tar_gz(
    plan: &Plan,
    destination: &Path,
    level: CompressionLevel,
    progress: &mut dyn FnMut(&CreateProgress),
    cancel: &AtomicBool,
) -> Result<u64> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::{Builder, EntryType, Header};

    let file = File::create(destination)?;
    let encoder = GzEncoder::new(BufWriter::new(file), Compression::new(level.deflate()));
    let mut builder = Builder::new(encoder);

    let mut processed = 0u64;
    for (index, entry) in plan.entries.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }
        progress(&CreateProgress {
            current: entry.name.clone(),
            processed,
            total: plan.total,
            index,
            count: plan.entries.len(),
        });
        let mut header = Header::new_gnu();
        header.set_mtime(entry.modified.unwrap_or(0));
        if entry.is_dir {
            header.set_entry_type(EntryType::Directory);
            header.set_mode(0o755);
            header.set_size(0);
            builder
                .append_data(&mut header, format!("{}/", entry.name), io::empty())
                .map_err(|error| ArchiveError::new(format!("写入 {} 失败：{error}", entry.name)))?;
            continue;
        }
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(entry.bytes);
        let file = File::open(&entry.path)
            .map_err(|error| ArchiveError::new(format!("读取 {} 失败：{error}", entry.path.display())))?;
        builder
            .append_data(&mut header, &entry.name, ProgressReader {
                inner: file,
                on_bytes: &mut processed,
                progress,
                name: &entry.name,
                index,
                count: plan.entries.len(),
                total: plan.total,
                cancel,
            })
            .map_err(|error| ArchiveError::new(format!("写入 {} 失败：{error}", entry.name)))?;
    }
    let encoder = builder
        .into_inner()
        .map_err(|error| ArchiveError::new(format!("收尾失败：{error}")))?;
    let mut buffer = encoder
        .finish()
        .map_err(|error| ArchiveError::new(format!("收尾失败：{error}")))?;
    io::Write::flush(&mut buffer)?;
    Ok(processed)
}

/// Reports progress from inside `tar`'s copy loop, which reads the file itself.
struct ProgressReader<'a> {
    inner: File,
    on_bytes: &'a mut u64,
    progress: &'a mut dyn FnMut(&CreateProgress),
    name: &'a str,
    index: usize,
    count: usize,
    total: u64,
    cancel: &'a AtomicBool,
}

impl io::Read for ProgressReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let read = self.inner.read(buffer)?;
        if read > 0 {
            *self.on_bytes += read as u64;
            (self.progress)(&CreateProgress {
                current: self.name.to_owned(),
                processed: *self.on_bytes,
                total: self.total,
                index: self.index,
                count: self.count,
            });
        }
        Ok(read)
    }
}

fn write_seven_zip(
    plan: &Plan,
    destination: &Path,
    level: CompressionLevel,
    progress: &mut dyn FnMut(&CreateProgress),
    cancel: &AtomicBool,
) -> Result<u64> {
    use sevenz_rust::{SevenZArchiveEntry, SevenZMethod, SevenZWriter};

    let mut writer = SevenZWriter::create(destination)
        .map_err(|error| ArchiveError::new(format!("无法创建 7Z：{error}")))?;
    let method = if level == CompressionLevel::Store {
        SevenZMethod::COPY
    } else {
        SevenZMethod::LZMA2
    };
    writer.set_content_methods(vec![method.into()]);

    let mut processed = 0u64;
    for (index, entry) in plan.entries.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled_error());
        }
        progress(&CreateProgress {
            current: entry.name.clone(),
            processed,
            total: plan.total,
            index,
            count: plan.entries.len(),
        });
        let mut archive_entry = SevenZArchiveEntry::from_path(&entry.path, entry.name.clone());
        archive_entry.is_directory = entry.is_dir;
        archive_entry.has_stream = !entry.is_dir;
        if entry.is_dir {
            writer
                .push_archive_entry::<&[u8]>(archive_entry, None)
                .map_err(|error| ArchiveError::new(format!("写入 {} 失败：{error}", entry.name)))?;
            continue;
        }
        let file = File::open(&entry.path)
            .map_err(|error| ArchiveError::new(format!("读取 {} 失败：{error}", entry.path.display())))?;
        writer
            .push_archive_entry(archive_entry, Some(ProgressReader {
                inner: file,
                on_bytes: &mut processed,
                progress,
                name: &entry.name,
                index,
                count: plan.entries.len(),
                total: plan.total,
                cancel,
            }))
            .map_err(|error| ArchiveError::new(format!("写入 {} 失败：{error}", entry.name)))?;
    }
    writer
        .finish()
        .map_err(|error| ArchiveError::new(format!("收尾失败：{error}")))?;
    Ok(processed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mugenzip-create-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fixture(root: &Path) {
        fs::create_dir_all(root.join("inner")).unwrap();
        let mut text = File::create(root.join("a.txt")).unwrap();
        text.write_all(b"alpha").unwrap();
        let mut text = File::create(root.join("inner/b.txt")).unwrap();
        text.write_all(b"beta beta beta").unwrap();
    }

    /// Every format has to survive its own round trip: what we write, we read.
    fn round_trip(format: CreateFormat) {
        let work = scratch(&format!("round-{}", format.extension()).replace('.', "-"));
        let source = work.join("报告");
        fixture(&source);
        let request = CreateRequest {
            sources: vec![source],
            destination_dir: work.clone(),
            name: "报告".to_owned(),
            format,
            level: CompressionLevel::Balanced,
        };
        let cancel = AtomicBool::new(false);
        let report = create_archive(&request, &mut |_| {}, &cancel).unwrap();
        assert_eq!(report.entries, 4, "two files and two directories");
        assert_eq!(report.bytes, 5 + 14);
        assert!(report.path.is_file());

        let out = work.join("out");
        let mut reader = crate::open_archive(&report.path).unwrap();
        let extracted = reader
            .extract_entries(&out, &|_| true, false, &mut |_| {}, &cancel)
            .unwrap();
        assert_eq!(
            fs::read_to_string(out.join("报告/a.txt")).unwrap(),
            "alpha"
        );
        assert_eq!(
            fs::read_to_string(out.join("报告/inner/b.txt")).unwrap(),
            "beta beta beta"
        );
        assert_eq!(
            fs::canonicalize(&extracted.output_dir).unwrap(),
            fs::canonicalize(&out).unwrap()
        );
        let _ = fs::remove_dir_all(&work);
    }

    #[test]
    fn zip_round_trip() {
        round_trip(CreateFormat::Zip);
    }

    #[test]
    fn tar_gz_round_trip() {
        round_trip(CreateFormat::TarGz);
    }

    #[test]
    fn seven_zip_round_trip() {
        round_trip(CreateFormat::SevenZ);
    }

    #[test]
    fn repeated_names_do_not_overwrite() {
        let work = scratch("unique");
        fs::write(work.join("报告.zip"), b"old").unwrap();
        let next = unique_archive_path(&work, "报告", CreateFormat::Zip);
        assert_eq!(next.file_name().unwrap(), "报告 (2).zip");
        fs::write(&next, b"older").unwrap();
        let third = unique_archive_path(&work, "报告", CreateFormat::Zip);
        assert_eq!(third.file_name().unwrap(), "报告 (3).zip");
        let _ = fs::remove_dir_all(&work);
    }

    #[test]
    fn cancelling_leaves_nothing_behind() {
        let work = scratch("cancel");
        let source = work.join("big");
        fixture(&source);
        let cancel = AtomicBool::new(true);
        let request = CreateRequest {
            sources: vec![source],
            destination_dir: work.clone(),
            name: "big".to_owned(),
            format: CreateFormat::Zip,
            level: CompressionLevel::Fast,
        };
        let error = create_archive(&request, &mut |_| {}, &cancel).unwrap_err();
        assert!(error.is_cancelled());
        assert!(!work.join("big.zip").exists());
        let leftovers = fs::read_dir(&work)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".part"))
            .count();
        assert_eq!(leftovers, 0);
        let _ = fs::remove_dir_all(&work);
    }

    #[test]
    fn extension_comes_from_the_format() {
        let work = scratch("ext");
        let request = CreateRequest {
            sources: vec![work.join("nothing")],
            destination_dir: work.clone(),
            name: "带后缀.zip".to_owned(),
            format: CreateFormat::SevenZ,
            level: CompressionLevel::Balanced,
        };
        assert_eq!(
            unique_archive_path(&request.destination_dir, &request.name, CreateFormat::SevenZ)
                .file_name()
                .unwrap()
                .to_string_lossy(),
            "带后缀.7z"
        );
        let _ = fs::remove_dir_all(&work);
    }
}
