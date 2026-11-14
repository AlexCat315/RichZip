use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::AtomicBool;

use unrar_ng::{Archive as UnrarArchive, VolumeInfo};
use unrar_ng_sys as sys;
use widestring::WideCString;

use super::{
    Archive, ArchiveEntry, ArchiveError, EntryKind, EntryProgress, ExtractReport, ExtractionPlan,
    ExtractionProgress, Result,
};

/// `wchar_t` is signed on Unix, while `WideCString` takes the unsigned wide char
/// of the same width.
#[cfg(windows)]
type WideChar = u16;
#[cfg(not(windows))]
type WideChar = u32;

pub struct RarBackend {
    path: PathBuf,
}

impl RarBackend {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }
}

impl Archive for RarBackend {
    fn list(&mut self) -> Result<Vec<ArchiveEntry>> {
        let archive = UnrarArchive::new(&self.path)
            .open_for_listing()
            .map_err(|error| ArchiveError::new(format!("无法打开 RAR：{error}")))?;
        if archive.volume_info() != VolumeInfo::None {
            return Err(ArchiveError::new(
                "RAR 分卷读取将在 M2 提供，请先使用第一卷并准备全部分卷",
            ));
        }
        if archive.has_encrypted_headers() {
            return Err(ArchiveError::new("RAR 头部已加密；密码支持将在 M2 提供"));
        }

        let mut entries = Vec::new();
        for header in archive {
            let header =
                header.map_err(|error| ArchiveError::new(format!("RAR 目录已损坏：{error}")))?;
            if header.is_split() {
                return Err(ArchiveError::new(format!(
                    "{} 跨多个 RAR 分卷，M1 不输出部分结果",
                    header.filename.display()
                )));
            }
            if header.is_encrypted() {
                return Err(ArchiveError::new(format!(
                    "{} 已加密；RAR 密码支持将在 M2 提供",
                    header.filename.display()
                )));
            }
            let unix_type = header.file_attr & 0o170000;
            let kind = if header.is_directory() {
                EntryKind::Directory
            } else if unix_type == 0o120000 {
                EntryKind::Symlink
            } else {
                EntryKind::File
            };
            entries.push(ArchiveEntry {
                name: header.filename.to_string_lossy().into_owned(),
                size: header.unpacked_size,
                modified: format_dos_date(header.file_time),
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
        let raw = RawRar::open(&self.path)?;
        let mut state = EntryProgress::new(&entries, progress, cancel)?;
        let mut index = 0usize;

        while let Some(header) = raw.read_header()? {
            let safe = plan
                .entries()
                .get(index)
                .ok_or_else(|| ArchiveError::new("RAR 条目数量在提取过程中发生变化"))?;
            if safe.name != header.name {
                return Err(ArchiveError::new(format!(
                    "RAR 条目顺序与列表不一致：{}",
                    header.name
                )));
            }
            if safe.kind == EntryKind::Directory {
                raw.process(sys::RAR_SKIP)?;
                index += 1;
                continue;
            }

            state.start(&safe.name)?;
            let (_, temp) = plan.create_temp(safe.source_index)?;
            let mut callback = RarCallback {
                output: temp,
                written: 0,
                progress: &mut state,
                failure: None,
            };
            raw.set_callback(&mut callback);
            let result = raw.process(sys::RAR_TEST);
            raw.clear_callback();
            if let Some(error) = callback.failure.take() {
                return Err(error);
            }
            result?;
            if callback.written != safe.size {
                return Err(ArchiveError::new(format!(
                    "{} 的内容大小与目录声明不一致",
                    safe.name
                )));
            }
            drop(callback);
            state.check_cancel()?;
            plan.publish(safe.source_index)?;
            state.finish()?;
            index += 1;
        }

        if index != plan.entries().len() {
            return Err(ArchiveError::new("RAR 条目提前结束，压缩包可能已损坏"));
        }
        Ok(ExtractReport {
            output_dir: plan.output_dir().to_path_buf(),
            completed: state.completed(),
        })
    }
}

struct RawHeader {
    name: String,
}

struct RawRar {
    handle: *const sys::Handle,
}

impl RawRar {
    #[cfg(windows)]
    fn open(path: &Path) -> Result<Self> {
        let path = WideCString::from_os_str(path.as_os_str())
            .map_err(|_| ArchiveError::new("RAR 路径包含 NUL 字符"))?;
        let data = sys::OpenArchiveDataEx::new(path.as_ptr(), sys::RAR_OM_EXTRACT);
        let handle = unsafe { sys::RAROpenArchiveEx(&data) };
        if handle.is_null() || data.open_result != sys::ERAR_SUCCESS as u32 {
            return Err(ArchiveError::new(format!(
                "无法打开 RAR：{}",
                rar_error(data.open_result as i32)
            )));
        }
        if data.flags & (sys::ROADF_VOLUME | sys::ROADF_FIRSTVOLUME) != 0 {
            unsafe { sys::RARCloseArchive(handle) };
            return Err(ArchiveError::new("RAR 分卷读取将在 M2 提供"));
        }
        if data.flags & sys::ROADF_ENCHEADERS != 0 {
            unsafe { sys::RARCloseArchive(handle) };
            return Err(ArchiveError::new("RAR 头部已加密；密码支持将在 M2 提供"));
        }
        Ok(Self { handle })
    }

    #[cfg(not(windows))]
    fn open(_path: &Path) -> Result<Self> {
        Err(ArchiveError::new("当前 RAR 流式后端仅支持 Windows"))
    }

    fn read_header(&self) -> Result<Option<RawHeader>> {
        let header = sys::HeaderDataEx::default();
        let code = unsafe { sys::RARReadHeaderEx(self.handle, &header) };
        if code == sys::ERAR_END_ARCHIVE {
            return Ok(None);
        }
        if code != sys::ERAR_SUCCESS {
            return Err(ArchiveError::new(format!(
                "读取 RAR 条目失败：{}",
                rar_error(code)
            )));
        }
        let filename = std::ptr::addr_of!(header.filename_w).cast::<WideChar>();
        let name = unsafe { WideCString::from_ptr_truncate(filename, 1024) }.to_string_lossy();
        Ok(Some(RawHeader { name }))
    }

    fn process(&self, operation: i32) -> Result<()> {
        let code =
            unsafe { sys::RARProcessFileW(self.handle, operation, ptr::null(), ptr::null()) };
        if code == sys::ERAR_SUCCESS {
            Ok(())
        } else {
            Err(ArchiveError::new(format!(
                "RAR 解码或 CRC 校验失败：{}",
                rar_error(code)
            )))
        }
    }

    fn set_callback(&self, callback: &mut RarCallback<'_, '_>) {
        unsafe {
            sys::RARSetCallback(
                self.handle,
                Some(rar_process_callback),
                callback as *mut RarCallback<'_, '_> as sys::LPARAM,
            );
        }
    }

    fn clear_callback(&self) {
        unsafe { sys::RARSetCallback(self.handle, None, 0) };
    }
}

impl Drop for RawRar {
    fn drop(&mut self) {
        unsafe { sys::RARCloseArchive(self.handle) };
    }
}

struct RarCallback<'a, 'b> {
    output: File,
    written: u64,
    progress: &'a mut EntryProgress<'b>,
    failure: Option<ArchiveError>,
}

extern "C" fn rar_process_callback(
    message: sys::UINT,
    user_data: sys::LPARAM,
    p1: sys::LPARAM,
    p2: sys::LPARAM,
) -> i32 {
    if user_data == 0 {
        return -1;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let callback = unsafe { &mut *(user_data as *mut RarCallback<'_, '_>) };
        match message {
            sys::UCM_PROCESSDATA => {
                if p1 == 0 || p2 <= 0 {
                    return 0;
                }
                let bytes = unsafe { std::slice::from_raw_parts(p1 as *const u8, p2 as usize) };
                if let Err(error) = callback.output.write_all(bytes) {
                    callback.failure =
                        Some(ArchiveError::new(format!("写入 RAR 临时文件失败：{error}")));
                    return -1;
                }
                callback.written = callback.written.saturating_add(bytes.len() as u64);
                if let Err(error) = callback.progress.advance(bytes.len()) {
                    callback.failure = Some(error);
                    return -1;
                }
                0
            }
            sys::UCM_CHANGEVOLUME | sys::UCM_CHANGEVOLUMEW => {
                callback.failure =
                    Some(ArchiveError::new("RAR 需要其他分卷；分卷支持将在 M2 提供"));
                -1
            }
            sys::UCM_NEEDPASSWORD | sys::UCM_NEEDPASSWORDW => {
                callback.failure = Some(ArchiveError::new("RAR 密码支持将在 M2 提供"));
                -1
            }
            sys::UCM_LARGEDICT => 0,
            _ => 0,
        }
    }));
    result.unwrap_or(-1)
}

fn format_dos_date(time: u32) -> String {
    let year = 1980 + (time >> 25);
    let month = (time >> 21) & 0x0F;
    let day = (time >> 16) & 0x1F;
    if month == 0 || day == 0 {
        "—".into()
    } else {
        format!("{year:04}-{month:02}-{day:02}")
    }
}

fn rar_error(code: i32) -> &'static str {
    match code {
        sys::ERAR_NO_MEMORY => "内存不足",
        sys::ERAR_BAD_DATA => "数据或 CRC 校验失败",
        sys::ERAR_BAD_ARCHIVE | sys::ERAR_UNKNOWN_FORMAT => "压缩包损坏或格式不受支持",
        sys::ERAR_EOPEN | sys::ERAR_EREAD => "无法读取压缩包或缺少分卷",
        sys::ERAR_ECREATE | sys::ERAR_EWRITE => "无法写入输出文件",
        sys::ERAR_MISSING_PASSWORD | sys::ERAR_BAD_PASSWORD => "需要密码（M2 支持）",
        sys::ERAR_LARGE_DICT => "字典大小超出当前后端限制",
        _ => "未知 UnRAR 错误",
    }
}
