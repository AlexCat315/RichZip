use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use sevenz_rust::{Password, SevenZReader};

use super::{
    Archive, ArchiveEntry, ArchiveError, EntryKind, EntryProgress, ExtractReport, ExtractionPlan,
    ExtractionProgress, Result,
};

pub struct SevenZipBackend {
    path: PathBuf,
}

impl SevenZipBackend {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn open(&self) -> Result<SevenZReader<std::fs::File>> {
        SevenZReader::open(&self.path, Password::empty()).map_err(|error| {
            ArchiveError::new(format!(
                "无法读取 7Z：{error}；若压缩包已加密，密码支持将在 M2 提供"
            ))
        })
    }
}

impl Archive for SevenZipBackend {
    fn list(&mut self) -> Result<Vec<ArchiveEntry>> {
        let reader = self.open()?;
        Ok(reader
            .archive()
            .files
            .iter()
            .filter(|entry| !entry.is_anti_item)
            .map(|entry| {
                let kind = if entry.is_directory {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                };
                ArchiveEntry {
                    name: entry.name.clone(),
                    size: entry.size,
                    modified: "—".into(),
                    kind,
                }
            })
            .collect())
    }

    fn extract(
        &mut self,
        destination: &Path,
        progress: &mut dyn FnMut(ExtractionProgress),
        cancel: &AtomicBool,
    ) -> Result<ExtractReport> {
        let entries = self.list()?;
        let plan = ExtractionPlan::new(&self.path, destination, &entries)?;
        let mut reader = self.open()?;
        let mut state = EntryProgress::new(&entries, progress, cancel)?;
        let mut failure = None;
        let mut index = 0usize;

        let result = reader.for_each_entries(|entry, mut input| {
            if entry.is_anti_item {
                return Ok(true);
            }
            let current = index;
            index += 1;
            let operation = (|| -> Result<bool> {
                let safe = plan
                    .entries()
                    .get(current)
                    .ok_or_else(|| ArchiveError::new("7Z 条目顺序在提取过程中发生变化"))?;
                if safe.name != entry.name {
                    return Err(ArchiveError::new("7Z 条目顺序与列表不一致，已停止提取"));
                }
                if safe.kind == EntryKind::Directory {
                    return Ok(true);
                }
                state.start(&safe.name)?;
                let (_, mut temp) = plan.create_temp(safe.source_index)?;
                state.copy(&mut input, &mut temp, safe.size)?;
                drop(temp);
                state.check_cancel()?;
                plan.publish(safe.source_index)?;
                state.finish()?;
                Ok(true)
            })();
            match operation {
                Ok(continue_reading) => Ok(continue_reading),
                Err(error) => {
                    failure = Some(error);
                    Ok(false)
                }
            }
        });

        if let Some(error) = failure {
            return Err(error);
        }
        result.map_err(|error| ArchiveError::new(format!("7Z 解码失败：{error}")))?;
        Ok(ExtractReport {
            output_dir: plan.output_dir().to_path_buf(),
            completed: state.completed(),
        })
    }
}
