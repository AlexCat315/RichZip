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

    fn extract(
        &mut self,
        destination: &Path,
        progress: &mut dyn FnMut(ExtractionProgress),
        cancel: &AtomicBool,
    ) -> Result<ExtractReport> {
        let entries = self.list()?;
        let plan = ExtractionPlan::new(&self.path, destination, &entries)?;
        let mut archive = self.open()?;
        let mut state = EntryProgress::new(&entries, progress, cancel)?;

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
        })
    }
}
