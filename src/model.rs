use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cove_ui::compose::prelude::{TableSort, TextBuffer};
use cove_ui::compose::primitives::interaction::DroppedFile;
use cove_ui::composition::DynamicField;
use cove_ui::core::State;
use cove_ui::core::scene::{ClipboardHolder, FilePicker, FilePickerFilter, FolderPicker, OpenedDocuments};
use cove_ui::core::work;
use cove_ui::hosts::desktop::application::composition::AppDisplayHandleHolder;

use mugenzip_core as engine;
use mugenzip_core::{
    self, ArchiveEntry, ArchiveError, ExtractReport, ExtractionProgress, format_size, open_archive,
};

use crate::browser::{self, ArchiveBrowser, FileItem};

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

impl LoadedArchive {
    /// "ZIP · 8 项 · 24 B" — the one line of metadata the header carries.
    pub fn meta_line(&self) -> String {
        format!(
            "{} · {} 项 · {}",
            self.format,
            self.entry_count,
            format_size(self.total_bytes)
        )
    }

    pub fn folder_dir(&self) -> Option<PathBuf> {
        self.path.parent().map(|parent| parent.to_path_buf())
    }
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

/// One finished extraction: the package, where it landed, and when.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtractRecord {
    pub name: String,
    pub destination: String,
    pub at: String,
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
    /// What has already been unpacked, newest first — package and落点 together,
    /// because "解到哪了" is the question this answers.
    pub history: State<Vec<ExtractRecord>>,
    pub directory: State<String>,
    pub selection: State<BTreeSet<String>>,
    /// The folder highlighted in the sidebar outline.
    pub tree_selection: State<Option<String>>,
    pub search: State<TextBuffer>,
    pub table_sort: State<Option<TableSort>>,
    /// Where the next extraction lands. Always visible in the action bar, so
    /// the user never has to guess what "解压" is about to do.
    pub destination: State<Option<PathBuf>>,
    /// Wrap the payload in a folder named after the archive, deduplicated.
    pub wrap_in_folder: State<bool>,
    pub phase: State<Phase>,
    pub progress: State<Progress>,
    pub status: State<Status>,
    pub drop_targeted: State<bool>,
    /// Where the divider rests, as a fraction of the body width. The user drags
    /// it; nothing else in the app writes it.
    pub sidebar_fraction: State<f32>,
    /// Archives opened this session, newest first, for the empty state.
    pub recent: State<Vec<PathBuf>>,
    task: Rc<RefCell<TaskSlot>>,
    sender: work::UiSender<TaskEvent>,
    hooks_installed: Cell<bool>,
}

impl AppModel {
    pub fn new() -> Self {
        let archive = State::new(None);
        let history = State::new(Vec::new());
        let history_for_events = history.clone();
        let directory = State::new(String::new());
        let selection = State::new(BTreeSet::new());
        let tree_selection = State::new(Some(String::new()));
        let search = State::new(TextBuffer::empty());
        let table_sort = State::new(None);
        let destination = State::new(None);
        let wrap_in_folder = State::new(true);
        let phase = State::new(Phase::Idle);
        let progress = State::new(Progress::default());
        let status = State::new(Status::default());
        let drop_targeted = State::new(false);
        let sidebar_fraction = State::new(0.20);
        let recent = State::new(Vec::new());
        let task = Rc::new(RefCell::new(TaskSlot::default()));

        let sender = {
            let archive = archive.clone();
            let directory = directory.clone();
            let selection = selection.clone();
            let tree_selection = tree_selection.clone();
            let destination = destination.clone();
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
                    &tree_selection,
                    &destination,
                    &history_for_events,
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
            history,
            directory,
            selection,
            tree_selection,
            search,
            table_sort,
            destination,
            wrap_in_folder,
            phase,
            progress,
            status,
            drop_targeted,
            sidebar_fraction,
            recent,
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
            DynamicField::state("history", &self.history),
            DynamicField::state("directory", &self.directory),
            DynamicField::state("selection", &self.selection),
            DynamicField::state("treeSelection", &self.tree_selection),
            DynamicField::state("search", &self.search),
            DynamicField::state("tableSort", &self.table_sort),
            DynamicField::state("destination", &self.destination),
            DynamicField::state("wrapInFolder", &self.wrap_in_folder),
            DynamicField::state("phase", &self.phase),
            DynamicField::state("progress", &self.progress),
            DynamicField::state("status", &self.status),
            DynamicField::state("dropTargeted", &self.drop_targeted),
            DynamicField::state("sidebarFraction", &self.sidebar_fraction),
            DynamicField::state("recent", &self.recent),
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

    /// Takes the first archive among the dropped files, ignoring the rest.
    pub fn open_dropped(&self, files: Vec<DroppedFile>) {
        if let Some(file) = files.into_iter().find(|file| !file.is_directory) {
            self.open(PathBuf::from(file.path.clone()));
        }
    }

    pub fn is_busy(&self) -> bool {
        !matches!(self.phase.get(), Phase::Idle | Phase::Ready)
    }

    /// The current directory's entries, before the search filter is applied.
    pub fn current_items(&self) -> Vec<FileItem> {
        match self.archive.get() {
            Some(archive) => archive.browser.items(&self.directory.get()),
            None => Vec::new(),
        }
    }

    /// The rows the table shows: the current directory narrowed by the search
    /// query. Column sorting is handled by the table itself.
    pub fn rows(&self) -> Vec<FileItem> {
        let query = self.search.get().string_value().to_lowercase();
        let items = self.current_items();
        if query.is_empty() {
            return items;
        }
        items
            .into_iter()
            .filter(|item| item.name.to_lowercase().contains(&query))
            .collect()
    }

    /// The selected entries in the current directory, for the action bar.
    pub fn selected_items(&self) -> Vec<FileItem> {
        let selection = self.selection.get();
        self.current_items()
            .into_iter()
            .filter(|item| selection.contains(&item.id))
            .collect()
    }

    /// The entries the current selection would extract, resolved through the
    /// archive so a selected folder counts its whole subtree.
    pub fn selection_scope(&self) -> (usize, u64) {
        let selection = self.selection.get();
        if selection.is_empty() {
            let archive = self.archive.get();
            return match archive {
                Some(archive) => (archive.entry_count, archive.total_bytes),
                None => (0, 0),
            };
        }
        let prefixes: Vec<String> = selection.iter().map(|id| format!("{id}/")).collect();
        let mut count = 0;
        let mut bytes = 0_u64;
        if let Some(archive) = self.archive.get() {
            for entry in archive.browser.all_entries() {
                let path = &entry.0;
                if selection.contains(path) || prefixes.iter().any(|p| path.starts_with(p)) {
                    count += 1;
                    bytes = bytes.saturating_add(entry.1);
                }
            }
        }
        (count, bytes)
    }

    /// "解压全部 8 项" / "解压选中 3 项" — the primary button's label.
    pub fn action_label(&self) -> String {
        let selected = self.selection.get().len();
        if selected == 0 {
            let count = self.archive.get().map_or(0, |archive| archive.entry_count);
            format!("解压全部 {count} 项")
        } else {
            format!("解压选中 {selected} 项")
        }
    }

    /// The destination as the bar shows it: home-relative, never a raw path.
    pub fn destination_label(&self) -> String {
        let Some(destination) = self.destination.get() else {
            return "未选择目标位置".into();
        };
        abbreviate_home(&destination)
    }

    pub fn set_destination(&self, path: PathBuf) {
        self.destination.set(Some(path));
    }

    pub fn choose_destination(&self) {
        if self.is_busy() {
            return;
        }
        let current = self.destination.get();
        let fallback = current
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(|| self.archive.get().and_then(|archive| {
                archive.folder_dir().map(|dir| dir.to_string_lossy().into_owned())
            }));
        let model = self.clone();
        FolderPicker::request(fallback.as_deref(), Box::new(move |chosen| {
            if let Some(directory) = chosen {
                model.set_destination(PathBuf::from(directory));
            }
        }));
    }

    pub fn toggle_wrap(&self) {
        let wrap = self.wrap_in_folder.get();
        self.wrap_in_folder.set(!wrap);
    }

    /// The single extraction entry point used by the action bar.
    pub fn extract(&self) {
        let Some(destination) = self.destination.get() else {
            self.choose_destination();
            return;
        };
        let target = if self.wrap_in_folder.get() {
            match self.archive.get() {
                Some(archive) => {
                    let base = engine::archive_base_name(&archive.path);
                    engine::unique_output_dir(&destination.join(base))
                }
                None => destination,
            }
        } else {
            destination
        };
        self.run_extract(target, self.selection.get());
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
        self.selection.set(BTreeSet::new());
        self.search.set(TextBuffer::empty());
        self.destination.set(path.parent().map(|parent| parent.to_path_buf()));
        self.phase.set(Phase::Loading);
        self.drop_targeted.set(false);
        self.push_recent(path.clone());
        self.status.set(Status {
            text: "正在读取".into(),
            detail: path.display().to_string(),
            tone: Tone::Busy,
        });

        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = load_archive(&path).map_err(|error| error.to_string());
            sender.send(TaskEvent::Loaded { generation, result });
        });
    }

    fn push_recent(&self, path: PathBuf) {
        let mut recent = self.recent.get();
        recent.retain(|candidate| candidate != &path);
        recent.insert(0, path);
        recent.truncate(4);
        self.recent.set(recent);
    }

    /// Re-opens an archive listed in the empty state.
    pub fn open_recent(&self, path: PathBuf) {
        self.open(path);
    }

    /// Extraction of one entry (and its subtree) into the visible destination.
    pub fn extract_item(&self, id: String) {
        let Some(destination) = self.destination.get() else {
            return;
        };
        let target = if self.wrap_in_folder.get() {
            match self.archive.get() {
                Some(archive) => {
                    let base = engine::archive_base_name(&archive.path);
                    engine::unique_output_dir(&destination.join(base))
                }
                None => destination,
            }
        } else {
            destination
        };
        self.run_extract(target, BTreeSet::from([id]));
    }

    fn run_extract(&self, destination: PathBuf, selected: BTreeSet<String>) {
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
            text: "正在解压".into(),
            detail: abbreviate_home(&destination),
            tone: Tone::Busy,
        });

        let sender = self.sender.clone();
        let source = archive.path.clone();
        std::thread::spawn(move || {
            let predicate = selection_filter(selected);
            let mut throttle = Throttle::new();
            let result = open_archive(&source).and_then(|mut archive| {
                archive.extract_entries(
                    &destination,
                    &predicate,
                    false,
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

    /// The落点 the sidebar offers: the package's own folder first, then the
    /// host's known folders. A destination picker, not a browser.
    pub fn destination_choices(&self) -> Vec<(String, PathBuf)> {
        let mut choices: Vec<(String, PathBuf)> = Vec::new();
        if let Some(archive) = self.archive.get()
            && let Some(folder) = archive.folder_dir()
        {
            let label = if self.wrap_in_folder.get() {
                format!("当前文件夹/{}", engine::archive_base_name(&archive.path))
            } else {
                "当前文件夹".to_owned()
            };
            choices.push((label, folder));
        }
        for (label, path) in browser::known_dirs() {
            if !choices.iter().any(|(_, existing)| existing == &path) {
                choices.push((label, path));
            }
        }
        choices
    }

    /// True when this choice is the one currently selected.
    pub fn destination_is(&self, path: &Path) -> bool {
        self.destination.get().as_deref() == Some(path)
    }

    /// The label the sidebar shows for the current target.
    pub fn destination_name(&self) -> String {
        let Some(destination) = self.destination.get() else {
            return "未选择".into();
        };
        for (label, path) in self.destination_choices() {
            if path == destination {
                return label;
            }
        }
        abbreviate_home(&destination)
    }

    /// Packages waiting behind the open one.
    pub fn queued(&self) -> Vec<PathBuf> {
        let current = self.archive.get().map(|archive| archive.path.clone());
        self.recent
            .get()
            .into_iter()
            .filter(|path| Some(path) != current.as_ref() && path.is_file())
            .take(3)
            .collect()
    }

    pub fn enter_directory(&self, directory: String) {
        let Some(archive) = self.archive.get() else {
            return;
        };
        if !archive.browser.contains_directory(&directory) {
            return;
        }
        self.tree_selection.set(Some(directory.clone()));
        self.directory.set(directory);
        self.selection.set(BTreeSet::new());
    }

    pub fn navigate_up(&self) {
        let parent = browser::parent_directory(&self.directory.get());
        self.enter_directory(parent);
    }

    pub fn activate(&self, item: &FileItem) {
        if item.folder {
            self.enter_directory(item.id.clone());
        } else {
            self.selection.set(BTreeSet::from([item.id.clone()]));
        }
    }

    pub fn select_all(&self) {
        let ids: BTreeSet<String> = self.current_items().into_iter().map(|item| item.id).collect();
        self.selection.set(ids);
    }

    pub fn copy_path(&self, id: String) {
        ClipboardHolder::write(&id);
        self.status.set(Status {
            text: "已复制路径".into(),
            detail: id,
            tone: Tone::Ok,
        });
    }
}

/// Builds the engine predicate for a selection. An empty selection means "the
/// whole archive"; otherwise an entry matches when it is selected or lives
/// under a selected folder.
fn selection_filter(selected: BTreeSet<String>) -> impl Fn(&ArchiveEntry) -> bool {
    let prefixes: Vec<String> = selected.iter().map(|id| format!("{id}/")).collect();
    move |entry| {
        if selected.is_empty() {
            return true;
        }
        let path = browser::normalize_path(&entry.name);
        selected.contains(&path) || prefixes.iter().any(|prefix| path.starts_with(prefix))
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
    selection: &State<BTreeSet<String>>,
    tree_selection: &State<Option<String>>,
    destination: &State<Option<PathBuf>>,
    history: &State<Vec<ExtractRecord>>,
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
                    let folder = loaded.folder_dir();
                    let summary = format!(
                        "{} 项 · {}",
                        loaded.entry_count,
                        format_size(loaded.total_bytes)
                    );
                    directory.set(String::new());
                    selection.set(BTreeSet::new());
                    tree_selection.set(Some(String::new()));
                    if destination.get().is_none() {
                        destination.set(folder);
                    }
                    set_window_title(&loaded.name);
                    archive.set(Some(Arc::new(loaded)));
                    phase.set(Phase::Ready);
                    status.set(Status {
                        text: "已打开".into(),
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
                    if let Some(archive) = archive.get() {
                        let mut records = history.get();
                        records.retain(|record| record.name != archive.name);
                        records.insert(
                            0,
                            ExtractRecord {
                                name: archive.name.clone(),
                                destination: abbreviate_home(&report.output_dir),
                                at: browser::format_timestamp(std::time::SystemTime::now()),
                            },
                        );
                        records.truncate(4);
                        history.set(records);
                    }
                    progress.set(Progress {
                        fraction: 1.0,
                        label: "100%".into(),
                        current_file: String::new(),
                    });
                    status.set(Status {
                        text: "解压完成".into(),
                        detail: if report.skipped == 0 {
                            format!(
                                "{} 个文件 · {}",
                                report.completed,
                                abbreviate_home(&report.output_dir)
                            )
                        } else {
                            format!(
                                "{} 个文件 · 跳过 {} 个链接 · {}",
                                report.completed,
                                report.skipped,
                                abbreviate_home(&report.output_dir)
                            )
                        },
                        tone: Tone::Ok,
                    });
                }
                Err(error) => {
                    let cancelled = error.is_cancelled();
                    status.set(Status {
                        text: if cancelled { "已取消" } else { "解压失败" }.into(),
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

/// `~/Downloads` instead of `/Users/someone/Downloads`: the action bar is
/// about the location, not the machine's home directory.
fn abbreviate_home(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = home {
        if let Ok(rest) = path.strip_prefix(&home) {
            if rest.as_os_str().is_empty() {
                return "~".into();
            }
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
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
