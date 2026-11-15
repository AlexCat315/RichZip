use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::engine::{ArchiveEntry, EntryKind, format_size};

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