//! In-place extraction for OS file-menu verbs.
//!
//! A verb that says "extract" should not open the archive: the browsing window
//! exists so a person can look inside and pick entries, and the verb already
//! knows what it wants. This runs the whole archive through the core extractor on
//! a worker thread, then reveals the result in Finder — the app never draws a
//! window for it.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use cove_ui::hosts::shell_services;

/// Where the contents land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Beside the archive, like Keka's "Extract here".
    Here,
    /// Into a folder named after the archive, numbered if it already exists.
    IntoFolder,
}

/// Extracts `archive` without the UI. Returns immediately: the work runs on its
/// own thread and the result is revealed in Finder when it finishes.
pub fn run(archive: PathBuf, placement: Placement) {
    std::thread::spawn(move || {
        let Some(parent) = archive.parent().map(Path::to_path_buf) else {
            return;
        };
        let destination = match placement {
            Placement::Here => parent,
            Placement::IntoFolder => {
                mugenzip_core::unique_output_dir(&parent.join(mugenzip_core::archive_base_name(&archive)))
            }
        };
        let Ok(mut reader) = mugenzip_core::open_archive(&archive) else {
            return;
        };
        let cancel = AtomicBool::new(false);
        // `smart` stays off: "here" means exactly this folder, and the folder case
        // already picked its own unique name, so bundling again would nest twice.
        // Finder junk never belongs in an output folder; the in-window job
        // follows the same rule, with a switch for people who disagree.
        if let Ok(report) = reader.extract_entries(
            &destination,
            &|entry| !mugenzip_core::is_system_entry(&entry.name),
            false,
            &mut |_| {},
            &cancel,
        ) {
            shell_services::reveal(&report.output_dir);
        }
    });
}
