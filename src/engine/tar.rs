use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use flate2::read::GzDecoder;

use super::{
    Archive, ArchiveEntry, ArchiveError, ArchiveFormat, EntryKind, EntryProgress, ExtractReport,
    ExtractionPlan, ExtractionProgress, Result,
};

pub struct TarBackend {
    path: PathBuf,
    format: ArchiveFormat,
}

impl TarBackend {
    pub fn new(path: &Path, format: ArchiveFormat) -> Self {
        Self {
            path: path.to_path_buf(),
            format,
        }
    }

    fn open(&self) -> Result<tar::Archive<Box<dyn Read>>> {
        let file = File::open(&self.path)
            .map_err(|error| ArchiveError::new(format!("无法打开 TAR：{error}")))?;
        let reader: Box<dyn Read> = if self.format == ArchiveFormat::TarGz {
            Box::new(GzDecoder::new(file))
        } else {
            Box::new(file)
        };
        Ok(tar::Archive::new(reader))
    }
}

impl Archive for TarBackend {
    fn list(&mut self) -> Result<Vec<ArchiveEntry>> {
        let mut archive = self.open()?;
        let entries = archive
            .entries()
            .map_err(|error| ArchiveError::new(format!("无法读取 TAR 目录：{error}")))?;
        let mut result = Vec::new();
        for entry in entries {
            let entry =
                entry.map_err(|error| ArchiveError::new(format!("TAR 条目已损坏：{error}")))?;
            let entry_type = entry.header().entry_type();
            let kind = if entry_type.is_dir() {
                EntryKind::Directory
            } else if entry_type.is_file() {
                EntryKind::File
            } else if entry_type.is_symlink() || entry_type.is_hard_link() {
                EntryKind::Symlink
            } else {
                EntryKind::Unsupported
            };
            let modified = entry
                .header()
                .mtime()
                .ok()
                .and_then(|time| format_unix_date(time as i64))
                .unwrap_or_else(|| "—".into());
            result.push(ArchiveEntry {
                name: entry
                    .path()
                    .map_err(|error| ArchiveError::new(format!("无法读取 TAR 条目路径：{error}")))?
                    .to_string_lossy()
                    .into_owned(),
                size: entry.size(),
                modified,
                kind,
            });
        }
        Ok(result)
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
        let tar_entries = archive
            .entries()
            .map_err(|error| ArchiveError::new(format!("无法读取 TAR 条目：{error}")))?;

        for (index, entry) in tar_entries.enumerate() {
            let mut entry = entry.map_err(|error| {
                ArchiveError::new(format!("读取 TAR 第 {} 个条目失败：{error}", index + 1))
            })?;
            let Some(safe) = plan.entry_at_source(index) else {
                continue;
            };
            if safe.kind == EntryKind::Directory {
                continue;
            }
            state.start(&safe.name)?;
            let (_, mut temp) = plan.create_temp(safe.source_index)?;
            state.copy(&mut entry, &mut temp, safe.size)?;
            drop(entry);
            drop(temp);
            state.check_cancel()?;
            plan.publish(safe.source_index)?;
            state.finish()?;
        }

        let mut trailing = archive.into_inner();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            state.check_cancel()?;
            if trailing
                .read(&mut buffer)
                .map_err(|error| ArchiveError::new(format!("TAR/GZ 校验失败：{error}")))?
                == 0
            {
                break;
            }
        }

        Ok(ExtractReport {
            output_dir: plan.output_dir().to_path_buf(),
            completed: state.completed(),
        })
    }
}

fn format_unix_date(seconds: i64) -> Option<String> {
    if seconds < 0 {
        return None;
    }
    let days = seconds.div_euclid(86_400);
    let z = days.checked_add(719_468)?;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    Some(format!("{year:04}-{month:02}-{day:02}"))
}
