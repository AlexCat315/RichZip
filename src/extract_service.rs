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
    /// If the archive has one top-level directory, extract beside it so that
    /// directory becomes the result (no `Archive/Archive/`). Otherwise use a
    /// named folder.
    Smart,
}

/// Extracts `archive` without the UI. Returns immediately: the work runs on its
/// own thread and the result is revealed in Finder when it finishes.
pub fn run(archive: PathBuf, placement: Placement) {
    std::thread::spawn(move || {
        if let Some(output) = run_blocking(archive, placement) {
            shell_services::reveal(&output);
        }
    });
}

/// Blocking form used by the context-menu command line, where the process must
/// stay alive until the output exists.
pub fn run_blocking(archive: PathBuf, placement: Placement) -> Option<PathBuf> {
    let parent = archive.parent().map(Path::to_path_buf)?;
    let mut reader = mugenzip_core::open_archive(&archive).ok()?;
    let base = mugenzip_core::archive_base_name(&archive);

    let destination = match placement {
        Placement::Here => parent.clone(),
        Placement::IntoFolder => mugenzip_core::unique_output_dir(&parent.join(base)),
        Placement::Smart => {
            let entries = reader.list().ok()?;
            let roots: std::collections::BTreeSet<String> = entries
                .iter()
                .filter(|entry| !mugenzip_core::is_system_entry(&entry.name))
                .filter_map(|entry| entry.name.split('/').next())
                .filter(|root| !root.is_empty())
                .map(str::to_owned)
                .collect();
            let single_root = roots.len() == 1;
            let root_is_directory = entries.iter().any(|entry| {
                entry.kind == mugenzip_core::EntryKind::Directory
                    && roots
                        .iter()
                        .next()
                        .is_some_and(|root| entry.name == *root)
            });
            if single_root && root_is_directory {
                parent.clone()
            } else {
                mugenzip_core::unique_output_dir(&parent.join(base))
            }
        }
    };

    let cancel = AtomicBool::new(false);
    // `smart` stays off: "here" means exactly this folder, and the folder case
    // already picked its own unique name, so bundling again would nest twice.
    // Finder junk never belongs in an output folder; the in-window job follows
    // the same rule, with a switch for people who disagree.
    let report = reader
        .extract_entries(
            &destination,
            &|entry| !mugenzip_core::is_system_entry(&entry.name),
            false,
            &mut |_| {},
            &cancel,
        )
        .ok()?;
    Some(report.output_dir)
}
