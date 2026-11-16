//! "Compress here" for the OS file menu.
//!
//! Like the in-place extract verbs, this never opens the window: Finder already
//! knows the selection, and the answer is a ZIP beside it. Multiple items become
//! one archive named after the selection's folder logic: a single item keeps its
//! own name, several become 归档.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use cove_ui::hosts::shell_services;

/// Packs `items` beside the first item, in the format and level chosen as
/// defaults in Settings. Runs on its own thread and reveals the archive when
/// it finishes.
pub fn run(
    items: Vec<PathBuf>,
    format: mugenzip_core::CreateFormat,
    level: mugenzip_core::CompressionLevel,
) {
    if items.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let Some(parent) = items
            .first()
            .and_then(|item| item.parent())
            .map(Path::to_path_buf)
        else {
            return;
        };
        let name = archive_name(&items);
        let request = mugenzip_core::CreateRequest {
            sources: items,
            destination_dir: parent,
            name,
            format,
            level,
        };
        let cancel = AtomicBool::new(false);
        match mugenzip_core::create_archive(&request, &mut |_| {}, &cancel) {
            Ok(report) => shell_services::reveal(&report.path),
            Err(_) => {}
        }
    });
}

/// One folder keeps its name; one file loses its extension; a mixed selection
/// has no better name than 归档.
fn archive_name(items: &[PathBuf]) -> String {
    match items {
        [one] => {
            let path = if one.is_dir() {
                one.file_name()
            } else {
                one.file_stem()
            };
            path.map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "归档".into())
        }
        _ => "归档".into(),
    }
}
