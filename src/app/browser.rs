use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use crate::engine::{ArchiveEntry, EntryKind};
use crate::{BreadcrumbItem, FileItem};

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
                let item = browser.file_item(&entry, name, &path, false);
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
            name: self.archive_name.clone().into(),
            path: "".into(),
        }];
        let mut path = String::new();
        for component in directory
            .split('/')
            .filter(|component| !component.is_empty())
        {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(component);
            breadcrumbs.push(BreadcrumbItem {
                name: component.into(),
                path: path.clone().into(),
            });
        }
        breadcrumbs
    }

    pub fn child_directory(&self, directory: &str, index: usize) -> Option<String> {
        self.children.get(directory)?.get(index).and_then(|item| {
            if item.folder {
                Some(item.id.to_string())
            } else {
                None
            }
        })
    }

    pub fn contains_directory(&self, directory: &str) -> bool {
        self.directories.contains(directory)
    }

    fn ensure_directory(&mut self, directory: &str) {
        let mut current = String::new();
        for component in directory
            .split('/')
            .filter(|component| !component.is_empty())
        {
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
            id: path.into(),
            name: name.into(),
            ext: "".into(),
            size: "—".into(),
            modified: "—".into(),
            path: self.full_path(path).into(),
            folder: true,
        }
    }

    fn file_item(&self, entry: &ArchiveEntry, name: &str, path: &str, folder: bool) -> FileItem {
        let extension = Path::new(name)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_uppercase();
        FileItem {
            id: path.into(),
            name: name.into(),
            ext: extension.into(),
            size: crate::engine::format_size(entry.size).into(),
            modified: entry.modified.clone().into(),
            path: self.full_path(path).into(),
            folder,
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

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn split_parent(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}
