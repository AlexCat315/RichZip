use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cove_ui::compose::primitives::interaction::DroppedFile;
use cove_ui::composition::DynamicField;
use cove_ui::core::State;
use cove_ui::core::scene::{FilePicker, FilePickerFilter, FolderPicker, OpenedDocuments};
use cove_ui::core::work;
use cove_ui::hosts::desktop::application::composition::AppDisplayHandleHolder;

use crate::engine::{
    self, ArchiveError, ExtractReport, ExtractionProgress, format_size, open_archive,
};
use browser::{ArchiveBrowser, FileItem};

use super::browser;

pub const ARCHIVE_EXTENSIONS: [&str; 10] = [
    "zip", "7z", "rar", "tar", "gz", "tgz", "cbz", "cbr", "bz2", "xz",
];

#[derive(Clone, Debug)]
pub struct LoadedArchive {
    pub path: PathBuf,
    pub name: String,
    pub format: &'static str,
    pub browser: ArchiveBrowser,
    pub entry_count: usize,
    pub total_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Loading,
    Ready,
    Extracting,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Ok,
    Busy,
    Error,
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub fraction: f64,
    pub label: String,
    pub current_file: String,
}

#[derive(Clone, Debug)]
pub struct Status {
    pub text: String,
    pub detail: String,
    pub tone: Tone,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            text: "就绪".into(),
            detail: String::new(),
            tone: Tone::Ok,
        }
    }
}

/// The running task: its generation, its cancel flag, and the channel receiver
/// that keeps its UI callback registered.
#[derive(Default)]
struct TaskSlot {
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    receiver: Option<work::UiReceiver>,
    /// The OS asked to close while a task was running. The task's completion
    /// handler quits once the workspace is clean.
    close_requested: bool,
}

enum TaskEvent {
    Loaded {
        generation: u64,
        result: Result<LoadedArchive, String>,
    },
    Progress {
        generation: u64,
        progress: ExtractionProgress,
    },
    Extracted {
        generation: u64,
        result: Result<ExtractReport, ArchiveError>,
    },
}

#[derive(Clone)]
pub struct AppModel {
    pub archive: State<Option<Arc<LoadedArchive>>>,
    pub directory: State<String>,
    pub selection: State<Option<String>>,
    pub phase: State<Phase>,
    pub progress: State<Progress>,
    pub status: State<Status>,
    pub drop_targeted: State<bool>,
    task: Rc<RefCell<TaskSlot>>,
    sender: work::UiSender<TaskEvent>,
    hooks_installed: Cell<bool>,
}

impl AppModel {
    pub fn new() -> Self {
        let archive = State::new(None);
        let directory = State::new(String::new());
        let selection = State::new(None);
        let phase = State::new(Phase::Idle);
        let progress = State::new(Progress::default());
        let status = State::new(Status::default());
        let drop_targeted = State::new(false);
        let task = Rc::new(RefCell::new(TaskSlot::default()));

        let sender = {
            let archive = archive.clone();
            let directory = directory.clone();
            let selection = selection.clone();
            let phase = phase.clone();
            let progress = progress.clone();
            let status = status.clone();
            let task = task.clone();
            let events = task.clone();
            let (sender, receiver) = work::ui_channel(move |event| {
                apply_event(
                    event,
                    &events,
                    &archive,
                    &directory,
                    &selection,
                    &phase,
                    &progress,
                    &status,
                );
            });
            task.borrow_mut().receiver = Some(receiver);
            sender
        };

        Self {
            archive,
            directory,
            selection,
            phase,
            progress,
            status,
            drop_targeted,
            task,
            sender,
            hooks_installed: Cell::new(false),
        }
    }

    /// The states the root view recomposes on. Declaring them here is what
    /// subscribes the view: a `State` read in `body` alone does not.
    pub fn dynamic_fields(&self) -> Vec<DynamicField> {
        vec![
            DynamicField::state("archive", &self.archive),
            DynamicField::state("directory", &self.directory),
            DynamicField::state("selection", &self.selection),
            DynamicField::state("phase", &self.phase),
            DynamicField::state("progress", &self.progress),
            DynamicField::state("status", &self.status),
            DynamicField::state("dropTargeted", &self.drop_targeted),
        ]
    }

    /// The host installs its ambients after the launch declaration is built, so
    /// the first body is the earliest point they can be wired.
    pub fn ensure_platform_hooks(&self) {
        if self.hooks_installed.replace(true) {
            return;
        }
        self.install_platform_hooks();
    }

    /// Wires the ambients the host installed before this view was mounted:
    /// Finder "Open With", the OS close/quit veto, and the window caption.
    fn install_platform_hooks(&self) {
        let model = self.clone();
        OpenedDocuments::set_handler(Some(Rc::new(move |documents: Vec<String>| {
            if let Some(first) = documents.into_iter().next() {
                model.open(PathBuf::from(first));
            }
        })));

        if let Some(display) = AppDisplayHandleHolder::current() {
            let task = self.task.clone();
            let status = self.status.clone();
            display.set_window_close_interceptor(Some(Rc::new(move |_window| {
                let mut slot = task.borrow_mut();
                let Some(cancel) = slot.cancel.clone() else {
                    return true;
                };
                cancel.store(true, Ordering::Relaxed);
                slot.close_requested = true;
                drop(slot);
                status.set(Status {
                    text: "正在退出".into(),
                    detail: "正在取消任务并清理临时文件…".into(),
                    tone: Tone::Busy,
                });
                false
            })));
        }

        // A path on the command line opens at launch, the way `open -a` and
        // Windows' "Open with" deliver it. Finder's document event arrives
        // through OpenedDocuments instead.
        if let Some(argument) = std::env::args_os().nth(1) {
            let path = PathBuf::from(argument);
            if path.is_file() {
                self.open(path);
            }
        }
    }

    pub fn open_dialog(&self) {
        if self.is_busy() {
            return;
        }
        let filters = [FilePickerFilter::new(
            "压缩包",
            ARCHIVE_EXTENSIONS.iter().map(|ext| ext.to_string()),
        )];
        let model = self.clone();
        FilePicker::request(&filters, false, Box::new(move |paths| {
            if let Some(first) = paths.into_iter().next() {
                model.open(PathBuf::from(first));
            }
        }));
    }

    pub fn extract_dialog(&self) {
        if self.is_busy() {
            return;
        }
        let Some(archive) = self.archive.get() else {
            return;
        };
        let default_path = archive
            .path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned());
        let model = self.clone();
        FolderPicker::request(default_path.as_deref(), Box::new(move |chosen| {
            if let Some(directory) = chosen {
                model.extract(PathBuf::from(directory));
            }
        }));
    }

    /// Takes the first archive among the dropped files, ignoring the rest.
    pub fn open_dropped(&self, files: Vec<DroppedFile>) {
        if let Some(file) = files.into_iter().find(|file| !file.is_directory) {
            self.open(PathBuf::from(file.path.clone()));
        }
    }

    pub fn is_busy(&self) -> bool {
        !matches!(self.phase.get(), Phase::Idle | Phase::Ready)
    }

    pub fn rows(&self) -> Vec<FileItem> {
        match self.archive.get() {
            Some(archive) => archive.browser.items(&self.directory.get()),
            None => Vec::new(),
        }
    }

    pub fn selected_item(&self) -> Option<FileItem> {
        let id = self.selection.get()?;
        self.rows().into_iter().find(|item| item.id == id)
    }

    /// Starts a new generation, cancelling nothing that is already running: the
    /// caller checks [`Self::is_busy`] first.
    fn begin_task(&self) -> (u64, Arc<AtomicBool>) {
        let mut task = self.task.borrow_mut();
        task.generation += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        task.cancel = Some(cancel.clone());
        (task.generation, cancel)
    }

    pub fn open(&self, path: PathBuf) {
        if self.is_busy() {
            return;
        }
        let (generation, _cancel) = self.begin_task();
        self.archive.set(None);
        self.directory.set(String::new());
        self.selection.set(None);
        self.phase.set(Phase::Loading);
        self.drop_targeted.set(false);
        self.status.set(Status {
            text: "正在打开".into(),
            detail: path.display().to_string(),
            tone: Tone::Busy,
        });

        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = load_archive(&path).map_err(|error| error.to_string());
            sender.send(TaskEvent::Loaded { generation, result });
        });
    }

    pub fn extract(&self, destination: PathBuf) {
        if self.is_busy() {
            return;
        }
        let Some(archive) = self.archive.get() else {
            return;
        };
        let (generation, cancel) = self.begin_task();
        self.phase.set(Phase::Extracting);
        self.progress.set(Progress {
            fraction: 0.0,
            label: "正在准备…".into(),
            current_file: "正在校验归档路径…".into(),
        });
        self.status.set(Status {
            text: "正在提取".into(),
            detail: destination.display().to_string(),
            tone: Tone::Busy,
        });

        let sender = self.sender.clone();
        let source = archive.path.clone();
        std::thread::spawn(move || {
            let mut throttle = Throttle::new();
            let result = open_archive(&source).and_then(|mut archive| {
                archive.extract(
                    &destination,
                    &mut |progress| {
                        if throttle.should_send(&progress) {
                            sender.send(TaskEvent::Progress {
                                generation,
                                progress,
                            });
                        }
                    },
                    &cancel,
                )
            });
            sender.send(TaskEvent::Extracted { generation, result });
        });
    }

    pub fn cancel_extract(&self) {
        if let Some(cancel) = &self.task.borrow().cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        self.status.set(Status {
            text: "正在取消".into(),
            detail: "正在停止解码并清理本次临时文件…".into(),
            tone: Tone::Busy,
        });
    }

    pub fn enter_directory(&self, directory: String) {
        let Some(archive) = self.archive.get() else {
            return;
        };
        if !archive.browser.contains_directory(&directory) {
            return;
        }
        self.directory.set(directory);
        self.selection.set(None);
    }

    pub fn navigate_up(&self) {
        let parent = browser::parent_directory(&self.directory.get());
        self.enter_directory(parent);
    }

    pub fn activate(&self, item: &FileItem) {
        if item.folder {
            self.enter_directory(item.id.clone());
        }
    }
}

/// Keeps the UI inbox from filling with one message per 64 KiB read.
struct Throttle {
    last_sent: Instant,
    last_file: String,
}

impl Throttle {
    fn new() -> Self {
        Self {
            last_sent: Instant::now() - Duration::from_millis(100),
            last_file: String::new(),
        }
    }

    fn should_send(&mut self, progress: &ExtractionProgress) -> bool {
        let changed = progress.current != self.last_file;
        let elapsed = self.last_sent.elapsed() >= Duration::from_millis(50);
        let done = progress.processed == progress.total;
        if changed || elapsed || done {
            self.last_sent = Instant::now();
            self.last_file = progress.current.clone();
            true
        } else {
            false
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_event(
    event: TaskEvent,
    task: &Rc<RefCell<TaskSlot>>,
    archive: &State<Option<Arc<LoadedArchive>>>,
    directory: &State<String>,
    selection: &State<Option<String>>,
    phase: &State<Phase>,
    progress: &State<Progress>,
    status: &State<Status>,
) {
    let generation = match &event {
        TaskEvent::Loaded { generation, .. }
        | TaskEvent::Progress { generation, .. }
        | TaskEvent::Extracted { generation, .. } => *generation,
    };
    if task.borrow().generation != generation {
        return;
    }
    match event {
        TaskEvent::Loaded { result, .. } => {
            task.borrow_mut().cancel = None;
            match result {
                Ok(loaded) => {
                    let summary = format!(
                        "{} · {} 个项目 · 共 {}",
                        loaded.name,
                        loaded.entry_count,
                        format_size(loaded.total_bytes)
                    );
                    let first = loaded
                        .browser
                        .items("")
                        .into_iter()
                        .find(|item| !item.folder)
                        .map(|item| item.id);
                    directory.set(String::new());
                    selection.set(first);
                    set_window_title(&loaded.name);
                    archive.set(Some(Arc::new(loaded)));
                    phase.set(Phase::Ready);
                    status.set(Status {
                        text: "就绪".into(),
                        detail: summary,
                        tone: Tone::Ok,
                    });
                }
                Err(message) => {
                    archive.set(None);
                    phase.set(Phase::Idle);
                    status.set(Status {
                        text: "无法打开".into(),
                        detail: message,
                        tone: Tone::Error,
                    });
                }
            }
        }
        TaskEvent::Progress { progress: update, .. } => {
            let fraction = if update.total == 0 {
                0.0
            } else {
                (update.processed as f64 / update.total as f64).clamp(0.0, 1.0)
            };
            progress.set(Progress {
                fraction,
                label: format!(
                    "{} / {} · {:.0}%",
                    format_size(update.processed),
                    format_size(update.total),
                    fraction * 100.0
                ),
                current_file: if update.current.is_empty() {
                    "正在准备…".into()
                } else {
                    update.current
                },
            });
        }
        TaskEvent::Extracted { result, .. } => {
            task.borrow_mut().cancel = None;
            phase.set(Phase::Ready);
            match result {
                Ok(report) => {
                    progress.set(Progress {
                        fraction: 1.0,
                        label: "100%".into(),
                        current_file: String::new(),
                    });
                    status.set(Status {
                        text: "提取完成".into(),
                        detail: format!(
                            "已完成 {} 个文件，输出到 {}",
                            report.completed,
                            report.output_dir.display()
                        ),
                        tone: Tone::Ok,
                    });
                }
                Err(error) => {
                    let cancelled = error.is_cancelled();
                    status.set(Status {
                        text: if cancelled { "已取消" } else { "提取失败" }.into(),
                        detail: error.to_string(),
                        tone: if cancelled { Tone::Ok } else { Tone::Error },
                    });
                }
            }
        }
    }
    // The OS close was vetoed until the workspace was clean; now it is.
    let slot = task.borrow();
    if slot.close_requested && slot.cancel.is_none() {
        drop(slot);
        if let Some(display) = AppDisplayHandleHolder::current() {
            display.quit();
        }
    }
}

fn set_window_title(archive_name: &str) {
    if let Some(display) = AppDisplayHandleHolder::current() {
        display.set_window_title(&format!("{archive_name} — MugenZip"));
    }
}

fn load_archive(path: &Path) -> engine::Result<LoadedArchive> {
    let format = engine::detect_format(path)?;
    let mut archive = open_archive(path)?;
    let entries = archive.list()?;
    let entry_count = entries.len();
    let total_bytes = entries
        .iter()
        .filter(|entry| entry.is_file())
        .fold(0u64, |sum, entry| sum.saturating_add(entry.size));
    let name = file_name(path);
    let browser = ArchiveBrowser::new(name.clone(), entries);
    Ok(LoadedArchive {
        path: path.to_path_buf(),
        name,
        format: format.label(),
        browser,
        entry_count,
        total_bytes,
    })
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}