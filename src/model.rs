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
use cove_ui::core::{Binding, State};
use cove_ui::core::scene::{ClipboardHolder, FilePicker, FilePickerFilter, FolderPicker, OpenedDocuments};
use cove_ui::core::work;
use cove_ui::hosts::desktop::application::composition::AppDisplayHandleHolder;

use crate::places::{self, PlaceRole};
use mugenzip_core as engine;
use mugenzip_core::{
    self, ArchiveEntry, ArchiveError, CompressionLevel, CreateFormat, CreateProgress,
    CreateReport, CreateRequest, ExtractReport, ExtractionProgress, create_archive,
    format_size, open_archive,
};

use crate::browser::{self, ArchiveBrowser, FileItem};
use crate::preview::{self, PreviewKind, PreviewOutcome};
use crate::theme::ThemeMode;

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
    /// Writing a new archive. Same progress surface as extraction, so the
    /// window only ever shows one kind of work at a time.
    Compressing,
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
    /// Home-relative, for display.
    pub destination: String,
    /// The real output folder, for "在访达显示".
    pub output_dir: PathBuf,
    pub at: String,
}

/// One archive sitting in the folder currently being browsed. The window opens
/// on a location, not on a package: a folder holds several archives and the
/// user usually wants to pick one — or several — out of them.
#[derive(Clone, Debug, PartialEq)]
pub struct PackageItem {
    pub path: PathBuf,
    pub name: String,
    pub size: String,
    pub bytes: u64,
    pub modified: String,
    /// Where this package was last unpacked to, when it was this session.
    pub extracted_to: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub fraction: f64,
    pub label: String,
    pub current_file: String,
}

/// What the right-hand inspector is showing, independent of extraction work.
/// A selection is enough to leave `Hidden`; only a single file can become a
/// card. Nothing here touches the archive task, so peeking never blocks it.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum PreviewState {
    #[default]
    Hidden,
    Single(PreviewCard),
    Multi(MultiPreview),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MultiPreview {
    pub items: Vec<PreviewRow>,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewRow {
    pub id: String,
    pub name: String,
    pub size: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewCard {
    pub id: String,
    pub name: String,
    pub kind: PreviewKind,
    pub extension: String,
    pub size: String,
    pub modified: String,
    pub bytes: u64,
    pub content: PreviewContent,
    /// The extracted file the preview owns. Present as soon as the worker has
    /// one, even for formats we cannot draw, because double-click hands it to
    /// the system.
    pub file: Option<PathBuf>,
    pub directory: Option<PathBuf>,
    /// PDF only: show the page at 2× inside the preview viewport instead of
    /// shrinking a whole A4 page into 272 pt. Reset for every new selection.
    pub pdf_zoomed: bool,
}

impl PreviewCard {
    pub fn label(&self) -> &'static str {
        match &self.content {
            PreviewContent::Loading => "读取预览",
            _ => self.kind.label(),
        }
    }

    pub fn cleanup(&self) {
        if let Some(directory) = &self.directory {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PreviewContent {
    Loading,
    Image(PathBuf),
    Pdf(PathBuf),
    Text {
        content: String,
        truncated: bool,
    },
    Note {
        message: String,
        error: bool,
    },
}

#[derive(Default)]
struct PreviewSlot {
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
    receiver: Option<work::UiReceiver>,
    pending: Option<PreviewCard>,
}

struct PreviewEvent {
    generation: u64,
    result: Result<PreviewOutcome, String>,
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
    /// A run over several packages: one task, one thread, one summary.
    BatchFinished {
        generation: u64,
        ok: usize,
        failed: Vec<String>,
        records: Vec<ExtractRecord>,
    },
    CreateProgress {
        generation: u64,
        progress: CreateProgress,
    },
    Created {
        generation: u64,
        result: Result<CreateReport, ArchiveError>,
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
    /// The folder the window is looking at: 下载, 桌面, or anything the user
    /// added. The sidebar switches between these; the main area lists what is
    /// inside one of them.
    pub location: State<Option<PathBuf>>,
    /// The archives found in `location`, newest first.
    pub packages: State<Vec<PackageItem>>,
    /// The packages ticked in the list, for a batch extraction.
    /// Package paths ticked in the list, keyed by their string form so the
    /// table's selection binding can own them directly.
    pub package_selection: State<BTreeSet<String>>,
    /// The right-hand inspector. Owned by selection, never by a toolbar action.
    pub preview: State<PreviewState>,
    /// The settings sheet is open.
    pub settings_open: State<bool>,
    /// Show the output folder in the file manager once extraction finishes.
    pub reveal_after_extract: State<bool>,
    /// The new-archive sheet is open.
    pub create_open: State<bool>,
    /// What will go into the next archive. Order is the user's.
    pub create_sources: State<Vec<PathBuf>>,
    pub create_format: State<CreateFormat>,
    pub create_level: State<CompressionLevel>,
    /// What a new job starts with. The sheet above is the per-job choice; these
    /// are the defaults it seeds from, and what the file-menu verb uses.
    pub default_create_format: State<CreateFormat>,
    pub default_create_level: State<CompressionLevel>,
    /// Archive name, without the format's extension.
    pub create_name: State<TextBuffer>,
    /// Where it lands. `None` means "beside the first source".
    pub create_destination: State<Option<PathBuf>>,
    /// Light or dark appearance, chosen in the settings sheet. `None` resolves
    /// to the light paper theme until the user picks.
    pub theme_mode: State<Option<ThemeMode>>,
    task: Rc<RefCell<TaskSlot>>,
    sender: work::UiSender<TaskEvent>,
    preview_task: Rc<RefCell<PreviewSlot>>,
    preview_sender: work::UiSender<PreviewEvent>,
    /// A double-click that arrived before the preview finished extracting.
    pending_open: Rc<RefCell<Option<String>>>,
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
        let location = State::new(None);
        let packages = State::new(Vec::new());
        let packages_for_events = packages.clone();
        let package_selection = State::new(BTreeSet::new());
        let preview = State::new(PreviewState::Hidden);
        let settings_open = State::new(false);
        let reveal_after_extract = State::new(true);
        let create_open = State::new(false);
        let create_sources = State::new(Vec::new());
        let create_format = State::new(CreateFormat::Zip);
        let create_level = State::new(CompressionLevel::Balanced);
        let default_create_format = State::new(CreateFormat::Zip);
        let default_create_level = State::new(CompressionLevel::Balanced);
        let create_name = State::new(TextBuffer::empty());
        let create_destination = State::new(None::<PathBuf>);
        let create_for_events = CreateStates {
            open: create_open.clone(),
            sources: create_sources.clone(),
            name: create_name.clone(),
            destination: create_destination.clone(),
        };
        let theme_mode = State::new(Some(ThemeMode::System));
        let reveal_for_events = reveal_after_extract.clone();
        let location_for_events = location.clone();
        let task = Rc::new(RefCell::new(TaskSlot::default()));
        let preview_task = Rc::new(RefCell::new(PreviewSlot::default()));
        let pending_open = Rc::new(RefCell::new(None::<String>));

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
                    &location_for_events,
                    &packages_for_events,
                    &reveal_for_events,
                    &create_for_events,
                );
            });
            task.borrow_mut().receiver = Some(receiver);
            sender
        };

        let preview_sender = {
            let preview_for_events = preview.clone();
            let slot_for_events = preview_task.clone();
            let pending_for_events = pending_open.clone();
            let (sender, receiver) = work::ui_channel(move |event: PreviewEvent| {
                let mut slot = slot_for_events.borrow_mut();
                if slot.generation != event.generation {
                    return;
                }
                let card = slot.pending.take();
                slot.cancel = None;
                drop(slot);
                let Some(card) = card else {
                    return;
                };
                let state = preview_state_for(card, event.result);
                preview_for_events.set(state.clone());
                if let Some(id) = pending_for_events.borrow_mut().take() {
                    if let PreviewState::Single(card) = &state
                        && card.id == id
                        && let Some(file) = &card.file
                    {
                        crate::model::open_path(file);
                    }
                }
            });
            preview_task.borrow_mut().receiver = Some(receiver);
            sender
        };
        preview::sweep();

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
            location,
            packages,
            package_selection,
            preview,
            settings_open,
            reveal_after_extract,
            create_open,
            create_sources,
            create_format,
            create_level,
            default_create_format,
            default_create_level,
            create_name,
            create_destination,
            theme_mode,
            task,
            sender,
            preview_task,
            preview_sender,
            pending_open,
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
            DynamicField::state("location", &self.location),
            DynamicField::state("packages", &self.packages),
            DynamicField::state("packageSelection", &self.package_selection),
            DynamicField::state("preview", &self.preview),
            DynamicField::state("settingsOpen", &self.settings_open),
            DynamicField::state("revealAfterExtract", &self.reveal_after_extract),
            DynamicField::state("createOpen", &self.create_open),
            DynamicField::state("createSources", &self.create_sources),
            DynamicField::state("createFormat", &self.create_format),
            DynamicField::state("createLevel", &self.create_level),
            DynamicField::state("defaultCreateFormat", &self.default_create_format),
            DynamicField::state("defaultCreateLevel", &self.default_create_level),
            DynamicField::state("createName", &self.create_name),
            DynamicField::state("createDestination", &self.create_destination),
            DynamicField::state("themeMode", &self.theme_mode),
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

    // -----------------------------------------------------------------------
    // locations and the packages inside them
    // -----------------------------------------------------------------------

    /// The places the sidebar offers: whatever the host resolves for desktop,
    /// documents and downloads, plus the home folder. The roles decide the
    /// order; the labels are ours, so a localized folder name cannot change
    /// where the app looks.
    pub fn locations(&self) -> Vec<(String, PathBuf)> {
        places::known_places()
            .into_iter()
            .map(|place| (place.label.to_owned(), place.path))
            .collect()
    }

    /// First paint: open on something real instead of an empty frame. Later
    /// paints leave the user's choice alone. The preference is the folders a
    /// person actually keeps archives in, not the names they happen to have.
    pub fn bootstrap_location(&self) {
        if self.location.get().is_some() {
            return;
        }
        // Resolved once: each lookup can hit the registry or the XDG file.
        let places = places::known_places();
        let start = [PlaceRole::Downloads, PlaceRole::Desktop, PlaceRole::Documents]
            .into_iter()
            .chain(std::iter::once(PlaceRole::Home))
            .find_map(|role| {
                places
                    .iter()
                    .find(|place| place.role == role)
                    .map(|place| place.path.clone())
            })
            .or_else(|| places.first().map(|place| place.path.clone()))
            // Last resort: a launch with no home folder at all (a stripped
            // container) still opens somewhere writable instead of nowhere.
            .or_else(|| std::env::current_dir().ok());
        if let Some(start) = start {
            self.set_location(start);
        }
    }

    fn start_place(&self, role: PlaceRole) -> Option<PathBuf> {
        places::known_places()
            .into_iter()
            .find(|place| place.role == role)
            .map(|place| place.path)
    }

    pub fn set_location(&self, path: PathBuf) {
        self.location.set(Some(path));
        self.package_selection.set(BTreeSet::new());
        self.refresh_packages();
    }

    /// Re-reads the current folder. Called on location change and whenever a
    /// package's state could have moved (an extraction finished).
    pub fn refresh_packages(&self) {
        let Some(dir) = self.location.get() else {
            return;
        };
        let history = self.history.get();
        self.packages.set(scan_packages(&dir, &history));
    }

    /// The package rows after the search filter — the browse list's search box.
    pub fn visible_packages(&self) -> Vec<PackageItem> {
        let query = self.search.get().string_value().to_lowercase();
        let packages = self.packages.get();
        if query.is_empty() {
            return packages;
        }
        packages
            .into_iter()
            .filter(|item| item.name.to_lowercase().contains(&query))
            .collect()
    }

    /// Back out of a package to the folder it came from. The list is re-read
    /// on the way out so a fresh extraction shows up as 已解压.
    pub fn close_archive(&self) {
        if self.is_busy() {
            return;
        }
        self.archive.set(None);
        self.directory.set(String::new());
        self.clear_preview();
        self.selection.set(BTreeSet::new());
        self.search.set(TextBuffer::empty());
        self.phase.set(Phase::Idle);
        set_window_title("MugenZip");
        self.refresh_packages();
    }

    pub fn last_extraction(&self) -> Option<ExtractRecord> {
        self.history.get().into_iter().next()
    }

    /// "在访达显示" / "在资源管理器显示": open the folder the last run wrote to.
    pub fn reveal_last(&self) {
        if let Some(record) = self.last_extraction() {
            reveal_in_file_manager(&record.output_dir);
        }
    }

    pub fn choose_location(&self) {
        // The dialog opens on the folder already on screen; on a fresh launch
        // that is the bootstrap folder, so it never falls back to the OS's own
        // last-used directory.
        let fallback = self
            .location
            .get()
            .or_else(|| self.start_place(PlaceRole::Home))
            .map(|path| path.to_string_lossy().into_owned());
        let model = self.clone();
        FolderPicker::request(fallback.as_deref(), Box::new(move |chosen| {
            if let Some(directory) = chosen {
                model.set_location(PathBuf::from(directory));
            }
        }));
    }

    /// The name the toolbar shows: a known folder keeps its short label, a
    /// folder the user added shows its own last component.
    pub fn location_name(&self) -> String {
        let Some(location) = self.location.get() else {
            return "位置".into();
        };
        for (label, path) in self.locations() {
            if path == location {
                return label;
            }
        }
        file_name(&location)
    }

    /// "6 个压缩包 · 共 1.2 GB"
    pub fn location_summary(&self) -> String {
        let packages = self.packages.get();
        if packages.is_empty() {
            return "没有压缩包".into();
        }
        let bytes = packages.iter().map(|item| item.bytes).sum::<u64>();
        format!(
            "{} 个压缩包 · 共 {}",
            packages.len(),
            format_size(bytes)
        )
    }

    /// How many archives a location holds, for the sidebar count. Only the
    /// directory is read; nothing is opened.
    pub fn package_count(&self, directory: &Path) -> usize {
        count_archives(directory)
    }

    pub fn package_key(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    pub fn clear_packages(&self) {
        self.package_selection.set(BTreeSet::new());
    }

    pub fn selected_packages(&self) -> Vec<PackageItem> {
        let selection = self.package_selection.get();
        self.packages
            .get()
            .into_iter()
            .filter(|item| selection.contains(&Self::package_key(&item.path)))
            .collect()
    }

    pub fn package_scope(&self) -> (usize, u64) {
        let selected = self.selected_packages();
        (
            selected.len(),
            selected.iter().map(|item| item.bytes).sum::<u64>(),
        )
    }

    /// Unpacks every ticked package, one after another in a single task: a
    /// batch is one piece of work, so it gets one progress bar and one cancel.
    pub fn batch_extract(&self) {
        if self.is_busy() {
            return;
        }
        let selected = self.selected_packages();
        if selected.is_empty() {
            return;
        }
        let destination = match self.destination.get().or_else(|| self.default_destination()) {
            Some(destination) => destination,
            None => {
                self.choose_destination();
                return;
            }
        };

        let (generation, cancel) = self.begin_task();
        self.phase.set(Phase::Extracting);
        self.progress.set(Progress {
            fraction: 0.0,
            label: format!("0 / {} 项", selected.len()),
            current_file: "正在准备…".into(),
        });
        self.status.set(Status {
            text: "正在解压".into(),
            detail: format!("{} 个压缩包 · {}", selected.len(), abbreviate_home(&destination)),
            tone: Tone::Busy,
        });

        let sender = self.sender.clone();
        let wrap = self.wrap_in_folder.get();
        std::thread::spawn(move || {
            let total = selected.len() as u64;
            let mut ok = 0;
            let mut failed: Vec<String> = Vec::new();
            let mut records: Vec<ExtractRecord> = Vec::new();
            for (index, package) in selected.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                sender.send(TaskEvent::Progress {
                    generation,
                    progress: ExtractionProgress {
                        current: package.name.clone(),
                        processed: index as u64,
                        total,
                    },
                });
                let base = engine::archive_base_name(&package.path);
                let target = if wrap {
                    engine::unique_output_dir(&destination.join(base))
                } else {
                    destination.clone()
                };
                let result = open_archive(&package.path).and_then(|mut archive| {
                    archive.extract_entries(&target, &|_: &ArchiveEntry| true, false, &mut |_| {}, &cancel)
                });
                match result {
                    Ok(report) => {
                        ok += 1;
                        records.push(ExtractRecord {
                            name: package.name.clone(),
                            destination: abbreviate_home(&report.output_dir),
                            output_dir: report.output_dir.clone(),
                            at: browser::format_timestamp(std::time::SystemTime::now()),
                        });
                    }
                    Err(error) => {
                        if error.is_cancelled() {
                            break;
                        }
                        failed.push(format!("{}：{error}", package.name));
                    }
                }
            }
            sender.send(TaskEvent::BatchFinished {
                generation,
                ok,
                failed,
                records,
            });
        });
    }

    pub fn open_settings(&self) {
        self.settings_open.set(true);
    }

    pub fn close_settings(&self) {
        self.settings_open.set(false);
    }

    /// The active appearance, defaulting to light before any choice is made.
    pub fn theme_mode(&self) -> ThemeMode {
        self.theme_mode.get().unwrap_or_default()
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
        if files.is_empty() || self.is_busy() {
            return;
        }
        if self.create_open.get() {
            self.add_create_sources(files.into_iter().map(|file| PathBuf::from(file.path)).collect());
            return;
        }
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
        let Some(destination) = self.destination.get().or_else(|| self.default_destination()) else {
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

    // -- creating a new archive ---------------------------------------------

    /// Opens the sheet. Sources stay empty until the user adds something:
    /// guessing what to pack is how you end up compressing the wrong folder.
    pub fn open_create(&self) {
        if self.is_busy() {
            return;
        }
        // Every job starts from the defaults; the sheet can still change them
        // for this one job without touching what the next job uses.
        self.create_format.set(self.default_create_format.get());
        self.create_level.set(self.default_create_level.get());
        self.create_open.set(true);
        if self.create_name.get().is_empty() {
            self.create_name.set(TextBuffer::new(&self.suggested_create_name()));
        }
        if self.create_destination.get().is_none() {
            self.create_destination.set(self.default_create_destination());
        }
    }

    pub fn close_create(&self) {
        if self.is_busy() {
            return;
        }
        self.create_open.set(false);
    }

    /// The name the field starts with: the single source's stem, or "归档"
    /// when the archive holds several things.
    pub fn suggested_create_name(&self) -> String {
        let sources = self.create_sources.get();
        match sources.as_slice() {
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

    pub fn default_create_destination(&self) -> Option<PathBuf> {
        self.create_sources
            .get()
            .first()
            .and_then(|source| source.parent().map(Path::to_path_buf))
            .or_else(|| self.location.get())
            .or_else(places::home_dir)
    }

    pub fn create_destination_resolved(&self) -> Option<PathBuf> {
        self.create_destination
            .get()
            .or_else(|| self.default_create_destination())
    }

    pub fn create_destination_name(&self) -> String {
        self.create_destination_resolved()
            .map(|path| abbreviate_home(&path))
            .unwrap_or_else(|| "未选择".into())
    }

    pub fn choose_create_destination(&self) {
        if self.is_busy() {
            return;
        }
        let fallback = self
            .create_destination_resolved()
            .map(|path| path.to_string_lossy().into_owned());
        let model = self.clone();
        FolderPicker::request(fallback.as_deref(), Box::new(move |chosen| {
            if let Some(directory) = chosen {
                model.create_destination.set(Some(PathBuf::from(directory)));
            }
        }));
    }

    pub fn set_create_format(&self, format: CreateFormat) {
        self.create_format.set(format);
    }

    pub fn set_create_level(&self, level: CompressionLevel) {
        self.create_level.set(level);
    }

    pub fn set_default_create_format(&self, format: CreateFormat) {
        self.default_create_format.set(format);
    }

    pub fn set_default_create_level(&self, level: CompressionLevel) {
        self.default_create_level.set(level);
    }

    /// Adds files from the platform panel. Multiple selection is the point:
    /// packing one file at a time is what the Finder verb is for.
    pub fn add_create_files(&self) {
        let model = self.clone();
        FilePicker::request(&[], true, Box::new(move |paths| {
            model.add_create_sources(paths.into_iter().map(PathBuf::from).collect());
        }));
    }

    /// Folders come one at a time because the platform folder panel returns
    /// one; pressing the button again adds the next.
    pub fn add_create_folder(&self) {
        let fallback = self
            .location
            .get()
            .map(|path| path.to_string_lossy().into_owned());
        let model = self.clone();
        FolderPicker::request(fallback.as_deref(), Box::new(move |chosen| {
            if let Some(directory) = chosen {
                model.add_create_sources(vec![PathBuf::from(directory)]);
            }
        }));
    }

    pub fn add_create_sources(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() || self.is_busy() {
            return;
        }
        // A name that followed the previous source list is stale once the
        // list changes; a name the user typed is not.
        let previous_name = self.create_name.get().string_value();
        let previous_suggestion = self.suggested_create_name();
        let previous_destination = self.create_destination.get();
        let previous_default = self.default_create_destination();
        let mut sources = self.create_sources.get();
        for path in paths {
            if !sources.contains(&path) {
                sources.push(path);
            }
        }
        self.create_sources.set(sources);
        // The destination follows the sources only while it is still the
        // automatic one; a folder the user picked is never overwritten.
        if previous_destination.is_none() || previous_destination == previous_default {
            self.create_destination.set(self.default_create_destination());
        }
        if previous_name.is_empty() || previous_name == previous_suggestion {
            self.create_name.set(TextBuffer::new(&self.suggested_create_name()));
        }
        self.create_open.set(true);
    }

    pub fn remove_create_source(&self, index: usize) {
        let mut sources = self.create_sources.get();
        if index < sources.len() {
            sources.remove(index);
        }
        let now_empty = sources.is_empty();
        self.create_sources.set(sources);
        if now_empty {
            self.create_name.set(TextBuffer::empty());
        }
    }

    pub fn create_source_count(&self) -> usize {
        self.create_sources.get().len()
    }

    /// Starts the write. Everything is checked here, so the button can stay
    /// quiet instead of being disabled for reasons the user cannot see.
    pub fn create_now(&self) {
        if self.is_busy() {
            return;
        }
        let sources = self.create_sources.get();
        if sources.is_empty() {
            self.status.set(Status {
                text: "还没有选择内容".into(),
                detail: "先添加文件或文件夹".into(),
                tone: Tone::Error,
            });
            return;
        }
        let name = self.create_name.get().string_value().trim().to_owned();
        let name = if name.is_empty() {
            self.suggested_create_name()
        } else {
            name
        };
        let Some(destination) = self.create_destination_resolved() else {
            self.choose_create_destination();
            return;
        };
        let format = self.create_format.get();
        let level = self.create_level.get();
        let (generation, cancel) = self.begin_task();
        self.phase.set(Phase::Compressing);
        self.progress.set(Progress {
            fraction: 0.0,
            label: "正在准备…".into(),
            current_file: "正在读取来源…".into(),
        });
        self.status.set(Status {
            text: "正在压缩".into(),
            detail: format!("{} · {}", name, format.label()),
            tone: Tone::Busy,
        });

        let request = CreateRequest {
            sources,
            destination_dir: destination,
            name,
            format,
            level,
        };
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let mut throttle = Throttle::new();
            let result = create_archive(
                &request,
                &mut |progress| {
                    if throttle.should_send_create(&progress) {
                        sender.send(TaskEvent::CreateProgress {
                            generation,
                            progress: progress.clone(),
                        });
                    }
                },
                &cancel,
            );
            sender.send(TaskEvent::Created { generation, result });
        });
    }

    pub fn open(&self, path: PathBuf) {
        if self.is_busy() {
            return;
        }
        let (generation, _cancel) = self.begin_task();
        self.archive.set(None);
        self.directory.set(String::new());
        self.clear_preview();
        self.selection.set(BTreeSet::new());
        self.search.set(TextBuffer::empty());
        self.destination.set(path.parent().map(|parent| parent.to_path_buf()));
        if let Some(parent) = path.parent()
            && self.location.get().as_deref() != Some(parent)
        {
            // Opening a package moves the window to the folder it came from, so
            // the sidebar keeps telling the truth about where you are.
            self.set_location(parent.to_path_buf());
        }
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
        let compressing = self.phase.get() == Phase::Compressing;
        self.status.set(Status {
            text: "正在取消".into(),
            detail: if compressing {
                "正在停止编码并清理临时文件…".into()
            } else {
                "正在停止解码并清理本次临时文件…".into()
            },
            tone: Tone::Busy,
        });
    }

    /// The落点 the sidebar offers: the package's own folder first, then the
    /// host's known folders. A destination picker, not a browser.
    pub fn destination_choices(&self) -> Vec<(String, PathBuf)> {
        let mut choices: Vec<(String, PathBuf)> = Vec::new();
        if let Some(folder) = self.default_destination() {
            // The chip reads "解压到 HaHa", not "当前文件夹/HaHa": the folder
            // the package came from is the default, so its own name is enough.
            let label = if let Some(archive) = self.archive.get() {
                if self.wrap_in_folder.get() {
                    engine::archive_base_name(&archive.path)
                } else {
                    "当前文件夹".to_owned()
                }
            } else {
                self.location_name()
            };
            choices.push((label, folder));
        }
        for place in places::known_places() {
            if !choices.iter().any(|(_, existing)| existing == &place.path) {
                choices.push((place.label.to_owned(), place.path));
            }
        }
        choices
    }

    /// Where an extraction lands when the chip has not been used yet: the
    /// package's own folder, or — in the batch view — the folder on screen.
    pub fn default_destination(&self) -> Option<PathBuf> {
        if let Some(archive) = self.archive.get()
            && let Some(folder) = archive.folder_dir()
        {
            return Some(folder);
        }
        self.location.get()
    }

    /// The label the sidebar shows for the current target.
    pub fn destination_name(&self) -> String {
        let Some(destination) = self.destination.get().or_else(|| self.default_destination()) else {
            return "未选择".into();
        };
        for (label, path) in self.destination_choices() {
            if path == destination {
                return label;
            }
        }
        abbreviate_home(&destination)
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
        self.clear_preview();
        self.selection.set(BTreeSet::new());
    }

    pub fn navigate_up(&self) {
        let parent = browser::parent_directory(&self.directory.get());
        self.enter_directory(parent);
    }

    pub fn activate(&self, item: &FileItem) {
        if item.folder {
            self.enter_directory(item.id.clone());
            return;
        }
        // The click already selected this row; a double-click then means
        // "open with the system", but only once the peek has a real file.
        self.change_selection(BTreeSet::from([item.id.clone()]));
        self.open_selected_with_system(&item.id);
    }

    pub fn select_all(&self) {
        let ids: BTreeSet<String> = self.current_items().into_iter().map(|item| item.id).collect();
        self.change_selection(ids);
    }

    /// The table's selection binding. Every tick — a plain click, ⌘-click,
    /// shift range, or clearing — goes through here, which is why a single
    /// click is enough to open the inspector.
    pub fn selection_binding(&self) -> Binding<BTreeSet<String>> {
        let model = self.clone();
        Binding::new(
            {
                let selection = self.selection.clone();
                move || selection.get()
            },
            move |value| model.change_selection(value),
        )
    }

    pub fn change_selection(&self, value: BTreeSet<String>) {
        if self.selection.get() == value {
            return;
        }
        self.selection.set(value.clone());
        self.preview_selection(value);
    }

    /// Toggle the PDF page between whole-page fit and a 2× scrollable view.
    /// Only applies once the page is actually extracted; the worker's card is
    /// immutable while loading, so this cannot race with an incoming result.
    pub fn toggle_pdf_zoom(&self) {
        let PreviewState::Single(mut card) = self.preview.get() else {
            return;
        };
        if !matches!(card.content, PreviewContent::Pdf(_)) {
            return;
        }
        card.pdf_zoomed = !card.pdf_zoomed;
        self.preview.set(PreviewState::Single(card));
    }

    /// The reader sheet's own close path. It does not need the card to still
    /// be selected, only that a PDF card is open.
    pub fn close_pdf_reader(&self) {
        let PreviewState::Single(mut card) = self.preview.get() else {
            return;
        };
        if card.pdf_zoomed {
            card.pdf_zoomed = false;
            self.preview.set(PreviewState::Single(card));
        }
    }

    /// Select one file from inside the inspector's related list. The table
    /// follows because both read the same state.
    pub fn select_preview_id(&self, id: String) {
        self.change_selection(BTreeSet::from([id]));
    }

    fn preview_selection(&self, selected: BTreeSet<String>) {
        if selected.is_empty() {
            self.clear_preview();
            return;
        }
        let items: Vec<FileItem> = self
            .current_items()
            .into_iter()
            .filter(|item| selected.contains(&item.id))
            .collect();
        if items.is_empty() {
            self.clear_preview();
            return;
        }
        if items.len() > 1 || selected.len() > 1 {
            self.cancel_preview();
            let bytes = items.iter().map(|item| item.bytes).sum();
            self.preview.set(PreviewState::Multi(MultiPreview {
                items: items
                    .into_iter()
                    .map(|item| PreviewRow {
                        id: item.id,
                        name: item.name,
                        size: item.size,
                        bytes: item.bytes,
                    })
                    .collect(),
                bytes,
            }));
            return;
        }
        let item = &items[0];
        if item.folder {
            self.clear_preview();
            return;
        }
        self.start_preview(item.clone());
    }

    fn start_preview(&self, item: FileItem) {
        let Some(archive) = self.archive.get() else {
            return;
        };
        let kind = PreviewKind::for_extension(&item.ext);
        let mut card = PreviewCard {
            id: item.id.clone(),
            name: item.name.clone(),
            kind,
            extension: item.ext.clone(),
            size: item.size.clone(),
            modified: item.modified.clone(),
            bytes: item.bytes,
            content: PreviewContent::Loading,
            file: None,
            directory: None,
            pdf_zoomed: false,
        };

        // Refuse before extracting, not after: a 3 GB text entry should cost a
        // line of copy, not a decompression.
        let limit = if kind == PreviewKind::Text {
            4 * 1024 * 1024
        } else {
            64 * 1024 * 1024
        };
        if item.bytes > limit {
            card.content = PreviewContent::Note {
                message: format!(
                    "文件超过 {}，为避免卡顿没有建立预览。双击仍可交给系统打开。",
                    format_size(limit)
                ),
                error: false,
            };
            self.cancel_preview();
            self.preview.set(PreviewState::Single(card));
            return;
        }

        self.cancel_preview();
        let mut slot = self.preview_task.borrow_mut();
        slot.generation += 1;
        let generation = slot.generation;
        let cancel = Arc::new(AtomicBool::new(false));
        slot.cancel = Some(cancel.clone());
        slot.pending = Some(card.clone());
        drop(slot);

        self.preview.set(PreviewState::Single(card));
        let sender = self.preview_sender.clone();
        let source = archive.path.clone();
        let id = item.id.clone();
        std::thread::spawn(move || {
            let result = preview::load(&source, &id, kind, generation, &cancel);
            sender.send(PreviewEvent { generation, result });
        });
    }

    fn cancel_preview(&self) {
        if let PreviewState::Single(card) = self.preview.get() {
            card.cleanup();
        }
        let mut slot = self.preview_task.borrow_mut();
        slot.generation += 1;
        if let Some(cancel) = slot.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        slot.pending = None;
    }

    pub fn clear_preview(&self) {
        if let PreviewState::Single(card) = self.preview.get() {
            card.cleanup();
        }
        self.cancel_preview();
        self.preview.set(PreviewState::Hidden);
    }

    /// Double-click on a file: use the extracted preview file when it is
    /// already ready; otherwise remember the intent and let the callback open
    /// it the moment extraction lands.
    pub fn open_selected_with_system(&self, id: &str) {
        if let PreviewState::Single(card) = self.preview.get()
            && card.id == id
        {
            if let Some(file) = &card.file {
                open_path(file);
                return;
            }
            if matches!(card.content, PreviewContent::Loading) {
                *self.pending_open.borrow_mut() = Some(id.to_owned());
                return;
            }
            if let PreviewContent::Note { error: true, .. } = &card.content {
                self.status.set(Status {
                    text: "无法打开".into(),
                    detail: "预览提取失败，文件没有被写出来".into(),
                    tone: Tone::Error,
                });
                return;
            }
        }
        *self.pending_open.borrow_mut() = Some(id.to_owned());
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

    fn should_send_create(&mut self, progress: &CreateProgress) -> bool {
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

/// The new-archive states the event pump needs. Grouped because the pump
/// already takes a dozen states and the list only grows.
struct CreateStates {
    open: State<bool>,
    sources: State<Vec<PathBuf>>,
    name: State<TextBuffer>,
    destination: State<Option<PathBuf>>,
}

/// Turn a worker outcome into the inspector's card. The card itself was
/// created before the thread started, so its name and row metadata stay exact
/// even when the result arrives after the table moved on.
fn preview_state_for(mut card: PreviewCard, result: Result<PreviewOutcome, String>) -> PreviewState {
    match result {
        Ok(outcome) => {
            card.file = outcome.file.clone();
            card.directory = Some(outcome.directory.clone());
            card.content = match outcome.kind {
                PreviewKind::Image => outcome
                    .file
                    .map(PreviewContent::Image)
                    .unwrap_or_else(|| PreviewContent::Note {
                        message: "预览文件没有生成".into(),
                        error: true,
                    }),
                PreviewKind::Pdf => outcome
                    .raster
                    .or(outcome.file)
                    .map(PreviewContent::Pdf)
                    .unwrap_or_else(|| PreviewContent::Note {
                        message: "预览文件没有生成".into(),
                        error: true,
                    }),
                PreviewKind::Text => PreviewContent::Text {
                    content: outcome.text.unwrap_or_default(),
                    truncated: outcome.truncated,
                },
                PreviewKind::Unsupported => PreviewContent::Note {
                    message: "这个格式没有内嵌预览。双击会交给系统默认应用。".into(),
                    error: false,
                },
            };
            PreviewState::Single(card)
        }
        Err(message) => {
            card.content = PreviewContent::Note {
                message,
                error: true,
            };
            PreviewState::Single(card)
        }
    }
}

/// Hand a file to the host's default application. The preview file lives in
/// the app's private temp directory, so this never opens the archive itself.
pub(crate) fn open_path(path: &Path) {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", ""]).arg(path);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };
    let _ = command.spawn();
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
    location: &State<Option<PathBuf>>,
    packages: &State<Vec<PackageItem>>,
    reveal_after_extract: &State<bool>,
    create: &CreateStates,
) {
    let generation = match &event {
        TaskEvent::Loaded { generation, .. }
        | TaskEvent::Progress { generation, .. }
        | TaskEvent::Extracted { generation, .. }
        | TaskEvent::BatchFinished { generation, .. }
        | TaskEvent::CreateProgress { generation, .. }
        | TaskEvent::Created { generation, .. } => *generation,
    };
    if task.borrow().generation != generation {
        return;
    }
    // A finished package changes what its row says, so the location is re-read
    // whenever work lands.
    let refresh = |history: &State<Vec<ExtractRecord>>| {
        if let Some(directory) = location.get() {
            packages.set(scan_packages(&directory, &history.get()));
        }
    };
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
                                output_dir: report.output_dir.clone(),
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
                    refresh(history);
                    if reveal_after_extract.get() {
                        reveal_in_file_manager(&report.output_dir);
                    }
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
        TaskEvent::BatchFinished {
            ok,
            failed,
            records,
            ..
        } => {
            task.borrow_mut().cancel = None;
            phase.set(Phase::Ready);
            let cancelled = task.borrow().close_requested;
            progress.set(Progress {
                fraction: 1.0,
                label: "100%".into(),
                current_file: String::new(),
            });
            if !records.is_empty() {
                let mut existing = history.get();
                for record in records.iter().rev() {
                    existing.retain(|candidate| candidate.name != record.name);
                    existing.insert(0, record.clone());
                }
                existing.truncate(4);
                history.set(existing);
            }
            let text = if failed.is_empty() {
                format!("已解压 {ok} 个压缩包")
            } else {
                format!("已解压 {ok} 个 · {} 个失败", failed.len())
            };
            status.set(Status {
                text,
                detail: if failed.is_empty() {
                    records
                        .first()
                        .map(|record| record.destination.clone())
                        .unwrap_or_default()
                } else {
                    failed.join(" · ")
                },
                tone: if failed.is_empty() { Tone::Ok } else { Tone::Error },
            });
            refresh(history);
            let _ = cancelled;
        }
        TaskEvent::CreateProgress { progress: update, .. } => {
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
        TaskEvent::Created { result, .. } => {
            task.borrow_mut().cancel = None;
            phase.set(Phase::Idle);
            match result {
                Ok(report) => {
                    create.open.set(false);
                    create.sources.set(Vec::new());
                    create.name.set(TextBuffer::empty());
                    create.destination.set(None);
                    progress.set(Progress {
                        fraction: 1.0,
                        label: "100%".into(),
                        current_file: String::new(),
                    });
                    let name = report
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    status.set(Status {
                        text: "已创建压缩包".into(),
                        detail: format!(
                            "{} · {} 项 · {}",
                            name,
                            report.entries,
                            format_size(report.bytes)
                        ),
                        tone: Tone::Ok,
                    });
                    if let Some(directory) = report.path.parent() {
                        location.set(Some(directory.to_path_buf()));
                        refresh(history);
                    }
                    if reveal_after_extract.get() {
                        reveal_in_file_manager(&report.path);
                    }
                }
                Err(error) => {
                    status.set(Status {
                        text: "无法创建压缩包".into(),
                        detail: error.to_string(),
                        tone: Tone::Error,
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
    let home = places::home_dir();
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

/// Reads a folder and describes every archive in it. Names, sizes and times
/// come from the filesystem; whether a package has been unpacked comes from
/// this session's history, so the list never claims more than it knows.
fn scan_packages(directory: &Path, history: &[ExtractRecord]) -> Vec<PackageItem> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut packages: Vec<PackageItem> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(str::to_lowercase)?;
            if !ARCHIVE_EXTENSIONS.contains(&ext.as_str()) {
                return None;
            }
            let metadata = entry.metadata().ok();
            let bytes = metadata.as_ref().map(|meta| meta.len()).unwrap_or(0);
            let modified = metadata
                .and_then(|meta| meta.modified().ok())
                .map(browser::format_timestamp)
                .unwrap_or_else(|| "—".to_owned());
            let name = file_name(&path);
            let extracted_to = history
                .iter()
                .find(|record| record.name == name)
                .map(|record| record.destination.clone());
            Some(PackageItem {
                path,
                name,
                size: format_size(bytes),
                bytes,
                modified,
                extracted_to,
            })
        })
        .collect();
    packages.sort_by(|left, right| right.modified.cmp(&left.modified));
    packages
}

fn count_archives(directory: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            let path = entry.path();
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ARCHIVE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .count()
}

/// Opens the file manager at a finished extraction. The only OS integration in
/// the app that is fire-and-forget.
/// Show one package in the file manager, leaving it selected.
pub fn reveal_parent(path: &Path) {
    reveal_in_file_manager(path);
}

fn reveal_in_file_manager(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let target = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
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
