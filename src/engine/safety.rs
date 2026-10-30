use std::collections::HashMap;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{ArchiveEntry, ArchiveError, EntryKind, Result};

static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);

type ValidatedEntry = (usize, PathBuf, String, u64, EntryKind);

#[derive(Debug)]
pub struct SafeEntry {
    pub source_index: usize,
    pub name: String,
    pub size: u64,
    pub kind: EntryKind,
    pub relative: PathBuf,
    pub target: PathBuf,
}

#[derive(Debug)]
pub struct TaskWorkspace {
    path: PathBuf,
}

impl TaskWorkspace {
    fn new(root: &Path) -> Result<Self> {
        for _ in 0..100 {
            let id = NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!(".mugenzip-task-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(ArchiveError::new(format!(
                        "无法创建临时工作区 {}：{error}",
                        path.display()
                    )))
                }
            }
        }
        Err(ArchiveError::new(
            "无法创建唯一临时工作区，请清理输出目录中的 MugenZip 临时目录",
        ))
    }

    fn temp_path(&self, index: usize) -> PathBuf {
        self.path.join(format!("{index}.part"))
    }
}

impl Drop for TaskWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug)]
pub struct ExtractionPlan {
    entries: Vec<SafeEntry>,
    output_dir: PathBuf,
    workspace: TaskWorkspace,
}

impl ExtractionPlan {
    pub fn new(archive_path: &Path, destination: &Path, entries: &[ArchiveEntry]) -> Result<Self> {
        let validated = validate_entries(entries)?;
        let output_dir = smart_output_dir(archive_path, destination, &validated);
        fs::create_dir_all(&output_dir).map_err(|error| {
            ArchiveError::new(format!(
                "无法创建输出目录 {}：{error}",
                output_dir.display()
            ))
        })?;
        let output_dir = output_dir.canonicalize().map_err(|error| {
            ArchiveError::new(format!(
                "无法确认输出目录 {}：{error}",
                output_dir.display()
            ))
        })?;

        let mut safe_entries = Vec::with_capacity(validated.len());
        for (source_index, relative, name, size, kind) in validated {
            let target = output_dir.join(&relative);
            if kind == EntryKind::File && fs::symlink_metadata(&target).is_ok() {
                return Err(ArchiveError::new(format!(
                    "目标已存在，未覆盖：{}",
                    target.display()
                )));
            }
            safe_entries.push(SafeEntry {
                source_index,
                name,
                size,
                kind,
                relative,
                target,
            });
        }

        for entry in &safe_entries {
            let directory = if entry.kind == EntryKind::Directory {
                &entry.relative
            } else {
                entry.relative.parent().unwrap_or_else(|| Path::new(""))
            };
            create_secure_directories(&output_dir, directory)?;
        }

        let workspace = TaskWorkspace::new(&output_dir)?;
        Ok(Self {
            entries: safe_entries,
            output_dir,
            workspace,
        })
    }

    pub fn entries(&self) -> &[SafeEntry] {
        &self.entries
    }

    pub fn output_dir(&self) -> &Path {
        &self.output_dir
    }

    pub fn create_temp(&self, source_index: usize) -> Result<(PathBuf, File)> {
        let path = self.workspace.temp_path(source_index);
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                ArchiveError::new(format!("无法创建临时文件 {}：{error}", path.display()))
            })?;
        Ok((path, file))
    }

    pub fn publish(&self, source_index: usize) -> Result<()> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.source_index == source_index)
            .ok_or_else(|| ArchiveError::new("内部错误：找不到待发布的提取条目"))?;
        let temp = self.workspace.temp_path(source_index);
        if fs::symlink_metadata(&entry.target).is_ok() {
            return Err(ArchiveError::new(format!(
                "目标已存在，未覆盖：{}",
                entry.target.display()
            )));
        }
        if let Some(parent) = entry.target.parent() {
            let relative = parent
                .strip_prefix(&self.output_dir)
                .map_err(|_| ArchiveError::new("内部错误：输出路径不在提取根目录中"))?;
            create_secure_directories(&self.output_dir, relative)?;
        }

        #[cfg(windows)]
        fs::rename(&temp, &entry.target).map_err(|error| {
            ArchiveError::new(format!("无法发布文件 {}：{error}", entry.target.display()))
        })?;

        #[cfg(not(windows))]
        {
            fs::hard_link(&temp, &entry.target).map_err(|error| {
                ArchiveError::new(format!("无法发布文件 {}：{error}", entry.target.display()))
            })?;
            fs::remove_file(&temp).map_err(|error| {
                ArchiveError::new(format!("无法清理临时文件 {}：{error}", temp.display()))
            })?;
        }
        Ok(())
    }
}

fn validate_entries(entries: &[ArchiveEntry]) -> Result<Vec<ValidatedEntry>> {
    let mut validated = Vec::with_capacity(entries.len());
    let mut paths: HashMap<String, (EntryKind, String)> = HashMap::new();

    for (index, entry) in entries.iter().enumerate() {
        if entry.kind == EntryKind::Symlink || entry.kind == EntryKind::Unsupported {
            return Err(ArchiveError::new(format!(
                "{} 是链接或特殊条目，M1 拒绝提取",
                entry.name
            )));
        }
        let relative = safe_relative_path(&entry.name)?;
        let key = path_key(&relative);
        if let Some((previous_kind, previous_name)) = paths.get(&key) {
            if *previous_kind != EntryKind::Directory || entry.kind != EntryKind::Directory {
                return Err(ArchiveError::new(format!(
                    "条目路径冲突：{} 与 {}",
                    previous_name, entry.name
                )));
            }
        } else {
            paths.insert(key, (entry.kind, entry.name.clone()));
        }
        validated.push((index, relative, entry.name.clone(), entry.size, entry.kind));
    }

    for (_, relative, name, _, kind) in &validated {
        let mut parent = relative.parent();
        while let Some(path) = parent {
            if path.as_os_str().is_empty() {
                break;
            }
            if let Some((parent_kind, _)) = paths.get(&path_key(path)) {
                if *parent_kind == EntryKind::File {
                    return Err(ArchiveError::new(format!(
                        "文件与目录冲突：{} 的父路径已是文件",
                        name
                    )));
                }
            }
            parent = path.parent();
        }
        if *kind == EntryKind::Directory {
            for (_, other, other_name, _, _) in &validated {
                if other != relative && other.starts_with(relative) {
                    continue;
                }
                if other == relative && other_name != name {
                    return Err(ArchiveError::new(format!("目录路径大小写冲突：{name}")));
                }
            }
        }
    }
    Ok(validated)
}

fn safe_relative_path(name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.contains('\0') {
        return Err(ArchiveError::new("压缩包包含空路径或 NUL 字符"));
    }
    if name.starts_with('/') || name.starts_with('\\') {
        return Err(ArchiveError::new(format!("拒绝绝对路径：{name}")));
    }
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        return Err(ArchiveError::new(format!("拒绝带盘符的路径：{name}")));
    }

    let mut relative = PathBuf::new();
    for component in name.split(['/', '\\']) {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(ArchiveError::new(format!("拒绝路径穿越：{name}")));
        }
        if component.contains(':') {
            return Err(ArchiveError::new(format!("拒绝 ADS 或盘符路径：{name}")));
        }
        if component.ends_with([' ', '.']) || component.chars().any(|c| c < '\u{20}') {
            return Err(ArchiveError::new(format!("拒绝 Windows 非法路径：{name}")));
        }
        if is_reserved_name(component) {
            return Err(ArchiveError::new(format!("拒绝 NTFS 保留名称：{name}")));
        }
        relative.push(component);
    }
    if relative.as_os_str().is_empty() {
        return Err(ArchiveError::new(format!("路径清理后为空：{name}")));
    }
    Ok(relative)
}

fn is_reserved_name(component: &str) -> bool {
    let base = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn smart_output_dir(
    archive_path: &Path,
    destination: &Path,
    entries: &[ValidatedEntry],
) -> PathBuf {
    let mut roots = entries
        .iter()
        .filter_map(|(_, path, _, _, _)| path.components().next())
        .map(|component| component.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    if roots.len() <= 1 {
        return destination.to_path_buf();
    }
    destination.join(archive_base_name(archive_path))
}

fn archive_base_name(path: &Path) -> String {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let lower = name.to_lowercase();
    if lower.ends_with(".tar.gz") {
        return name[..name.len() - 7].to_owned();
    }
    if lower.ends_with(".tgz") {
        return name[..name.len() - 4].to_owned();
    }
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn create_secure_directories(root: &Path, relative: &Path) -> Result<()> {
    let canonical_root = root.canonicalize().map_err(|error| {
        ArchiveError::new(format!("无法确认输出根目录 {}：{error}", root.display()))
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(ArchiveError::new(format!(
                        "目录位置已被文件占用：{}",
                        current.display()
                    )));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| {
                    ArchiveError::new(format!("无法创建目录 {}：{error}", current.display()))
                })?;
            }
            Err(error) => {
                return Err(ArchiveError::new(format!(
                    "无法检查目录 {}：{error}",
                    current.display()
                )))
            }
        }
        let canonical = current.canonicalize().map_err(|error| {
            ArchiveError::new(format!("无法确认目录 {}：{error}", current.display()))
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(ArchiveError::new(format!(
                "目录链接越出输出根目录：{}",
                current.display()
            )));
        }
    }
    Ok(())
}
