use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use zip::ZipArchive;

use super::{
    Archive, ArchiveEntry, ArchiveError, EntryKind, EntryProgress, ExtractReport, ExtractionPlan,
    ExtractionProgress, Result,
};

pub struct ZipBackend {
    path: PathBuf,
}

impl ZipBackend {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn open(&self) -> Result<ZipArchive<File>> {
        let file = File::open(&self.path)
            .map_err(|error| ArchiveError::new(format!("无法打开 ZIP：{error}")))?;
        ZipArchive::new(file)
            .map_err(|error| ArchiveError::new(format!("ZIP 已损坏或格式不受支持：{error}")))
    }
}

impl Archive for ZipBackend {
    fn list(&mut self) -> Result<Vec<ArchiveEntry>> {
        let mut archive = self.open()?;
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let file = archive.by_index(index).map_err(|error| {
                ArchiveError::new(format!("读取 ZIP 第 {} 个条目失败：{error}", index + 1))
            })?;
            let name = file
                .name()
                .map_err(|error| ArchiveError::new(format!("ZIP 条目文件名无法解码：{error}")))?
                .into_owned();
            if file.encrypted() {
                return Err(ArchiveError::new(format!(
                    "{name} 已加密；ZIP 密码支持将在 M2 提供"
                )));
            }
            let modified = file
                .last_modified()
                .map(|date| {
                    let text = date.to_string();
                    text[..10].to_owned()
                })
                .unwrap_or_else(|| "—".into());
            let kind = if file.is_dir() {
                EntryKind::Directory
            } else if file.is_symlink() {
                EntryKind::Symlink
            } else {
                EntryKind::File
            };
            entries.push(ArchiveEntry {
                name,
                size: file.size(),
                modified,
                kind,
            });
        }
        Ok(entries)
    }

    fn extract_entries(
        &mut self,
        destination: &Path,
        select: &dyn Fn(&ArchiveEntry) -> bool,
        smart: bool,
        progress: &mut dyn FnMut(ExtractionProgress),
        cancel: &AtomicBool,
    ) -> Result<ExtractReport> {
        let entries = self.list()?;
        let plan = ExtractionPlan::with_selection(&self.path, destination, &entries, select, smart)?;
        let chosen: Vec<ArchiveEntry> = entries.iter().filter(|entry| select(entry)).cloned().collect();
        let mut archive = self.open()?;
        let mut state = EntryProgress::new(&chosen, progress, cancel)?;

        for safe in plan.entries() {
            if safe.kind == EntryKind::Directory {
                continue;
            }
            state.start(&safe.name)?;
            let mut file = archive
                .by_index(safe.source_index)
                .map_err(|error| ArchiveError::new(format!("无法读取 {}：{error}", safe.name)))?;
            let (_, mut temp) = plan.create_temp(safe.source_index)?;
            state
                .copy(&mut file, &mut temp, safe.size)
                .map_err(|error| {
                    if error.is_cancelled() {
                        error
                    } else {
                        ArchiveError::new(format!("解压 {} 失败：{error}", safe.name))
                    }
                })?;
            drop(file);
            drop(temp);
            state.check_cancel()?;
            plan.publish(safe.source_index)?;
            state.finish()?;
        }

        Ok(ExtractReport {
            output_dir: plan.output_dir().to_path_buf(),
            completed: state.completed(),
            skipped: plan.skipped(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;
    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mugenzip-test-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, bytes) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn extracts_only_selected_entries() {
        let root = temp_dir("subset");
        let archive_path = root.join("sample.zip");
        write_zip(
            &archive_path,
            &[
                ("alpha.txt", b"aaaaa"),
                ("beta.txt", b"bbbbbb"),
                ("gamma.txt", b"ccccccc"),
            ],
        );
        let destination = root.join("out");
        let cancel = AtomicBool::new(false);
        let report = ZipBackend::new(&archive_path)
            .extract_entries(
                &destination,
                &|entry| entry.name != "beta.txt",
                false,
                &mut |_| {},
                &cancel,
            )
            .unwrap();
        assert_eq!(report.completed, 2);
        assert!(destination.join("alpha.txt").is_file());
        assert!(destination.join("gamma.txt").is_file());
        assert!(!destination.join("beta.txt").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn links_are_skipped_instead_of_failing_the_job() {
        let root = temp_dir("symlink");
        let destination = root.join("out");
        let entries = vec![
            ArchiveEntry {
                name: "payload.txt".into(),
                size: 5,
                modified: "—".into(),
                kind: EntryKind::File,
            },
            ArchiveEntry {
                name: "payload-link".into(),
                size: 11,
                modified: "—".into(),
                kind: EntryKind::Symlink,
            },
        ];
        let plan =
            ExtractionPlan::with_selection(&root.join("links.zip"), &destination, &entries, &|_| {
                true
            }, false)
            .expect("a package with links still plans");
        assert_eq!(plan.skipped(), 1, "the link is counted, not fatal");
        assert_eq!(plan.entries().len(), 1, "the regular file stays in the plan");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn smart_bundles_multiple_roots_into_named_folder() {
        let root = temp_dir("smart");
        let archive_path = root.join("bundle.zip");
        write_zip(
            &archive_path,
            &[("docs/readme.txt", b"r"), ("src/main.rs", b"m")],
        );
        let destination = root.join("out");
        let cancel = AtomicBool::new(false);
        let report = ZipBackend::new(&archive_path)
            .extract_entries(&destination, &|_| true, true, &mut |_| {}, &cancel)
            .unwrap();
        assert_eq!(report.output_dir, destination.join("bundle").canonicalize().unwrap());
        assert!(destination.join("bundle/docs/readme.txt").is_file());
        assert!(destination.join("bundle/src/main.rs").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn smart_single_root_extracts_in_place() {
        let root = temp_dir("single");
        let archive_path = root.join("flat.zip");
        write_zip(&archive_path, &[("only.txt", b"x")]);
        let destination = root.join("out");
        let cancel = AtomicBool::new(false);
        let report = ZipBackend::new(&archive_path)
            .extract_entries(&destination, &|_| true, true, &mut |_| {}, &cancel)
            .unwrap();
        assert_eq!(report.output_dir, destination.canonicalize().unwrap());
        assert!(destination.join("only.txt").is_file());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
