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

/// The C `tm` prefix every platform shares; only the first nine fields are
/// read, which is exactly what `localtime` fills in everywhere.
#[repr(C)]
#[derive(Default)]
struct LocalTm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
}

#[cfg(unix)]
unsafe extern "C" {
    fn localtime_r(time: *const i64, out: *mut LocalTm) -> *mut LocalTm;
}

#[cfg(windows)]
unsafe extern "C" {
    fn localtime(time: *const i64) -> *mut LocalTm;
}

/// Seconds the host adds to UTC, read once per call from the system zone.
/// A file row that says "04:54" has to be the clock the user reads, not GMT.
fn local_offset_seconds(epoch: i64) -> i64 {
    let mut tm = LocalTm::default();
    #[cfg(unix)]
    let filled = unsafe { localtime_r(&epoch, &mut tm) };
    #[cfg(windows)]
    let filled = unsafe { localtime(&epoch) };
    if filled.is_null() {
        return 0;
    }
    let year = tm.year as i64 + 1900;
    let month = tm.mon as i64 + 1;
    let day = tm.mday as i64;
    // Civil date back to days since 1970 (the inverse of the algorithm below).
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let local = days * 86_400
        + tm.hour as i64 * 3_600
        + tm.min as i64 * 60
        + tm.sec as i64;
    local - epoch
}

/// "2026-11-15 09:12" without pulling a date crate in for one column.
pub fn format_timestamp(time: std::time::SystemTime) -> String {
    const MONTHS: [&str; 12] = [
        "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
    ];
    let Ok(elapsed) = time.duration_since(std::time::UNIX_EPOCH) else {
        return "—".to_owned();
    };
    let epoch = elapsed.as_secs() as i64;
    let local = (epoch + local_offset_seconds(epoch)).max(0) as u64;
    let total_hours = local / 3600;
    let total_days = total_hours / 24;
    let hour = total_hours % 24;
    let minute = (local / 60) % 60;

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
