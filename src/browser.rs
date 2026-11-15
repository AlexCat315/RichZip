use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use mugenzip_core::{ArchiveEntry, EntryKind, format_size};

#[derive(Clone, Debug, PartialEq)]
pub struct FileItem {
    pub id: String,
    pub name: String,
    pub ext: String,
    pub size: String,
    pub modified: String,
    pub path: String,
    pub folder: bool,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreadcrumbItem {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub struct ArchiveBrowser {
    archive_name: String,
    children: BTreeMap<String, Vec<FileItem>>,
    directories: HashSet<String>,
}

impl ArchiveBrowser {
    pub fn new(archive_name: String, entries: Vec<ArchiveEntry>) -> Self {
        let mut browser = Self {
            archive_name,
            children: BTreeMap::new(),
            directories: HashSet::new(),
        };
        browser.children.insert(String::new(), Vec::new());
        browser.directories.insert(String::new());

        for entry in entries {
            let path = normalize_path(&entry.name);
            if path.is_empty() {
                continue;
            }
            if entry.kind == EntryKind::Directory {
                browser.ensure_directory(&path);
            } else {
                let (parent, name) = split_parent(&path);
                browser.ensure_directory(parent);
                let item = browser.file_item(&entry, name, &path);
                browser
                    .children
                    .entry(parent.to_owned())
                    .or_default()
                    .push(item);
            }
        }
        for items in browser.children.values_mut() {
            items.sort_by_cached_key(|item| {
                (
                    !item.folder,
                    item.name.to_lowercase(),
                    item.path.to_lowercase(),
                )
            });
        }
        browser
    }

    pub fn items(&self, directory: &str) -> Vec<FileItem> {
        self.children.get(directory).cloned().unwrap_or_default()
    }

    /// The synthetic root of the folder tree: the archive itself.
    pub fn archive_root_item(&self) -> FileItem {
        FileItem {
            id: String::new(),
            name: self.archive_name.clone(),
            ext: String::new(),
            size: "—".to_owned(),
            modified: "—".to_owned(),
            path: self.archive_name.clone(),
            folder: true,
            bytes: 0,
        }
    }

    /// Only the child folders of a directory, for the navigation tree.
    pub fn subfolders(&self, directory: &str) -> Vec<FileItem> {
        self.items(directory)
            .into_iter()
            .filter(|item| item.folder)
            .collect()
    }

    /// Every folder in the archive, flattened and depth-first, so the sidebar
    /// can list structure without repeating the file table's contents.
    pub fn folders(&self) -> Vec<FileItem> {
        let mut folders: Vec<FileItem> = Vec::new();
        for path in self.children.keys() {
            if path.is_empty() {
                continue;
            }
            let name = path.rsplit('/').next().unwrap_or(path).to_owned();
            folders.push(FileItem {
                id: path.clone(),
                name,
                ext: String::new(),
                size: "—".to_owned(),
                modified: "—".to_owned(),
                path: path.clone(),
                folder: true,
                bytes: 0,
            });
        }
        folders.sort_by_cached_key(|item| item.path.to_lowercase());
        folders
    }

    /// Every file entry with its byte size, for selection accounting.
    pub fn all_entries(&self) -> Vec<(String, u64)> {
        let mut entries = Vec::new();
        for items in self.children.values() {
            for item in items {
                if !item.folder {
                    entries.push((item.id.clone(), item.bytes));
                }
            }
        }
        entries
    }

    pub fn breadcrumbs(&self, directory: &str) -> Vec<BreadcrumbItem> {
        let mut breadcrumbs = vec![BreadcrumbItem {
            name: self.archive_name.clone(),
            path: String::new(),
        }];
        let mut path = String::new();
        for component in directory.split('/').filter(|part| !part.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(component);
            breadcrumbs.push(BreadcrumbItem {
                name: component.to_owned(),
                path: path.clone(),
            });
        }
        breadcrumbs
    }

    pub fn contains_directory(&self, directory: &str) -> bool {
        self.directories.contains(directory)
    }

    fn ensure_directory(&mut self, directory: &str) {
        let mut current = String::new();
        for component in directory.split('/').filter(|part| !part.is_empty()) {
            let parent = current.clone();
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(component);
            if self.directories.insert(current.clone()) {
                let item = self.folder_item(component, &current);
                self.children.entry(parent).or_default().push(item);
                self.children.entry(current.clone()).or_default();
            }
        }
    }

    fn folder_item(&self, name: &str, path: &str) -> FileItem {
        FileItem {
            id: path.to_owned(),
            name: name.to_owned(),
            ext: String::new(),
            size: "—".to_owned(),
            modified: "—".to_owned(),
            path: self.full_path(path),
            folder: true,
            bytes: 0,
        }
    }

    fn file_item(&self, entry: &ArchiveEntry, name: &str, path: &str) -> FileItem {
        let extension = Path::new(name)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_uppercase();
        FileItem {
            id: path.to_owned(),
            name: name.to_owned(),
            ext: extension,
            size: format_size(entry.size),
            modified: entry.modified.clone(),
            path: self.full_path(path),
            folder: false,
            bytes: entry.size,
        }
    }

    fn full_path(&self, path: &str) -> String {
        if path.is_empty() {
            self.archive_name.clone()
        } else {
            format!("{} / {path}", self.archive_name)
        }
    }
}

pub fn parent_directory(directory: &str) -> String {
    directory
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or("")
        .to_owned()
}

pub fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn split_parent(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// The host's own "save to" folders. Same idea on every platform, different
/// names: Finder's Desktop/Documents/Downloads, Explorer's Desktop/Documents/
/// Downloads, GNOME's XDG equivalents. Missing ones are simply not offered.
pub fn known_dirs() -> Vec<(String, PathBuf)> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let Some(home) = home else {
        return Vec::new();
    };
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();

    #[cfg(target_os = "macos")]
    {
        dirs.push(("桌面".to_owned(), home.join("Desktop")));
        dirs.push(("下载".to_owned(), home.join("Downloads")));
        dirs.push(("文稿".to_owned(), home.join("Documents")));
    }

    #[cfg(target_os = "windows")]
    {
        dirs.push(("桌面".to_owned(), home.join("Desktop")));
        dirs.push(("下载".to_owned(), home.join("Downloads")));
        dirs.push(("文档".to_owned(), home.join("Documents")));
    }

    #[cfg(target_os = "linux")]
    {
        dirs.push(("桌面".to_owned(), home.join("Desktop")));
        dirs.push(("下载".to_owned(), home.join("Downloads")));
        dirs.push(("文档".to_owned(), home.join("Documents")));
    }

    dirs.retain(|(_, path)| path.is_dir());
    dirs
}

/// "2026-11-15 09:12" without pulling a date crate in for one column.
pub fn format_timestamp(time: std::time::SystemTime) -> String {
    const MONTHS: [&str; 12] = [
        "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
    ];
    let Ok(elapsed) = time.duration_since(std::time::UNIX_EPOCH) else {
        return "—".to_owned();
    };
    let total_hours = elapsed.as_secs() / 3600;
    let total_days = total_hours / 24;
    let hour = total_hours % 24;
    let minute = (elapsed.as_secs() / 60) % 60;

    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = total_days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    let index = (month - 1).clamp(0, 11) as usize;
    format!(
        "{year}-{}-{:02} {:02}:{:02}",
        MONTHS[index], day, hour, minute
    )
}
