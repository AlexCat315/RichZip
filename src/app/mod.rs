mod browser;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slint::winit_030::{winit, EventResult, WinitWindowAccessor};
use slint::{ComponentHandle, ModelRc, VecModel, Weak};

use crate::engine::{detect_format, format_size, open_archive, ArchiveError, ExtractionProgress};
use crate::{FileItem, MainWindow};
use browser::{parent_directory, ArchiveBrowser};

#[derive(Default)]
struct Controller {
    archive: Option<PathBuf>,
    browser: Option<ArchiveBrowser>,
    directory: String,
    cancel: Option<Arc<AtomicBool>>,
    close_requested: bool,
    task_id: u64,
}

type SharedController = Arc<Mutex<Controller>>;

pub fn run() -> Result<(), slint::PlatformError> {
    let app = MainWindow::new()?;
    let state = Arc::new(Mutex::new(Controller::default()));

    {
        let weak = app.as_weak();
        app.on_move_window(move |dx, dy| {
            if let Some(app) = weak.upgrade() {
                let window = app.window();
                let scale = window.scale_factor();
                let position = window.position();
                window.set_position(slint::PhysicalPosition::new(
                    position.x + (dx * scale) as i32,
                    position.y + (dy * scale) as i32,
                ));
            }
        });
    }
    {
        let weak = app.as_weak();
        app.on_minimize_clicked(move || {
            if let Some(app) = weak.upgrade() {
                app.window().set_minimized(true);
            }
        });
    }
    {
        let weak = app.as_weak();
        let state = state.clone();
        app.on_close_clicked(move || {
            if let Some(app) = weak.upgrade() {
                request_close(&app, &state);
            }
        });
    }
    {
        let weak = app.as_weak();
        let state = state.clone();
        app.on_open_archive(move || {
            let busy = state
                .lock()
                .map(|state| state.task_is_busy())
                .unwrap_or(true);
            if busy {
                return;
            }
            let path = rfd::FileDialog::new()
                .add_filter(
                    "压缩包",
                    &["zip", "7z", "rar", "tar", "gz", "tgz", "cbz", "cbr"],
                )
                .add_filter("所有文件", &["*"])
                .pick_file();
            if let (Some(path), Some(app)) = (path, weak.upgrade()) {
                begin_open(&app, &state, path);
            }
        });
    }
    {
        let weak = app.as_weak();
        let state = state.clone();
        app.on_extract_to(move || {
            let archive = state.lock().ok().and_then(|state| {
                if state.task_is_busy() {
                    None
                } else {
                    state.archive.clone()
                }
            });
            let Some(archive) = archive else {
                return;
            };
            let destination = rfd::FileDialog::new().pick_folder();
            if let (Some(destination), Some(app)) = (destination, weak.upgrade()) {
                begin_extract(&app, &state, archive, destination);
            }
        });
    }
    {
        let weak = app.as_weak();
        let state = state.clone();
        app.on_cancel_extract(move || {
            if let Ok(state) = state.lock() {
                if let Some(cancel) = &state.cancel {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
            if let Some(app) = weak.upgrade() {
                app.set_status_text("正在取消".into());
                app.set_status_detail("正在停止解码并清理本次临时文件…".into());
            }
        });
    }
    {
        let weak = app.as_weak();
        let state = state.clone();
        app.on_activate_item(move |index| {
            if let Some(app) = weak.upgrade() {
                activate_item(&app, &state, index);
            }
        });
    }
    {
        let weak = app.as_weak();
        let state = state.clone();
        app.on_navigate_to(move |path| {
            if let Some(app) = weak.upgrade() {
                show_directory(&app, &state, path.to_string());
            }
        });
    }
    {
        let weak = app.as_weak();
        let state = state.clone();
        app.on_navigate_up(move || {
            if let Some(app) = weak.upgrade() {
                let parent = state.lock().map(|state| parent_directory(&state.directory));
                if let Ok(parent) = parent {
                    show_directory(&app, &state, parent);
                }
            }
        });
    }
    {
        let weak = app.as_weak();
        let state = state.clone();
        app.window()
            .on_winit_window_event(move |_, event| match event {
                winit::event::WindowEvent::DroppedFile(path) => {
                    if let Some(app) = weak.upgrade() {
                        app.set_drop_hover(false);
                        let busy = state
                            .lock()
                            .map(|state| state.task_is_busy())
                            .unwrap_or(true);
                        if !busy {
                            begin_open(&app, &state, path.clone());
                        }
                    }
                    EventResult::PreventDefault
                }
                winit::event::WindowEvent::HoveredFile(_) => {
                    if let Some(app) = weak.upgrade() {
                        let busy = state
                            .lock()
                            .map(|state| state.task_is_busy())
                            .unwrap_or(true);
                        app.set_drop_hover(!busy);
                    }
                    EventResult::PreventDefault
                }
                winit::event::WindowEvent::HoveredFileCancelled => {
                    if let Some(app) = weak.upgrade() {
                        app.set_drop_hover(false);
                    }
                    EventResult::PreventDefault
                }
                _ => EventResult::Propagate,
            });
    }

    if let Some(path) = std::env::args_os().nth(1).map(PathBuf::from) {
        begin_open(&app, &state, path);
    }
    app.run()
}

impl Controller {
    fn task_is_busy(&self) -> bool {
        self.cancel.is_some() || self.close_requested
    }
}

fn request_close(app: &MainWindow, state: &SharedController) {
    let should_wait = if let Ok(mut state) = state.lock() {
        state.close_requested = true;
        if let Some(cancel) = &state.cancel {
            cancel.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    } else {
        false
    };
    if should_wait {
        app.set_status_text("正在退出".into());
        app.set_status_detail("正在取消任务并清理临时文件…".into());
    } else {
        let _ = app.hide();
    }
}

fn begin_open(app: &MainWindow, state: &SharedController, path: PathBuf) {
    let task_id = {
        let Ok(mut controller) = state.lock() else {
            return;
        };
        controller.task_id += 1;
        controller.archive = None;
        controller.browser = None;
        controller.directory.clear();
        controller.cancel = Some(Arc::new(AtomicBool::new(false)));
        controller.close_requested = false;
        controller.task_id
    };
    app.set_archive_open(false);
    app.set_files(ModelRc::new(VecModel::from(Vec::<FileItem>::new())));
    app.set_breadcrumbs(ModelRc::new(VecModel::from(Vec::new())));
    app.set_can_navigate_up(false);
    app.set_selected(-1);
    app.set_archive_name(file_name(&path).into());
    app.set_archive_format("".into());
    app.set_archive_summary("正在读取压缩包…".into());
    app.set_busy(true);
    app.set_extracting(false);
    app.set_drop_hover(false);
    app.set_has_error(false);
    app.set_status_text("正在打开".into());
    app.set_status_detail(path.display().to_string().into());

    let weak = app.as_weak();
    let state = state.clone();
    std::thread::spawn(move || {
        let result = load_archive(&path);
        post(&weak, move |app| {
            let close = match state.lock() {
                Ok(mut controller) if controller.task_id == task_id => {
                    controller.cancel = None;
                    controller.close_requested
                }
                _ => return,
            };
            if close {
                let _ = app.hide();
                return;
            }
            app.set_busy(false);
            match result {
                Ok(LoadedArchive {
                    format,
                    browser,
                    files,
                    breadcrumbs,
                    entry_count,
                    total,
                }) => {
                    if let Ok(mut controller) = state.lock() {
                        controller.archive = Some(path.clone());
                        controller.browser = Some(browser);
                    }
                    let selected = files.iter().position(|item| !item.folder).unwrap_or(0);
                    let has_files = !files.is_empty();
                    app.set_archive_name(file_name(&path).into());
                    app.set_archive_format(format.into());
                    app.set_archive_summary(
                        format!(
                            "{} · {} 个项目 · 共 {}",
                            file_name(&path),
                            entry_count,
                            format_size(total)
                        )
                        .into(),
                    );
                    app.set_files(ModelRc::new(VecModel::from(files)));
                    app.set_breadcrumbs(ModelRc::new(VecModel::from(breadcrumbs)));
                    app.set_selected(if has_files { selected as i32 } else { -1 });
                    app.set_can_navigate_up(false);
                    app.set_archive_open(true);
                    app.set_status_text("就绪".into());
                    app.set_status_detail("".into());
                    app.set_has_error(false);
                }
                Err(error) => {
                    app.set_archive_open(false);
                    app.set_archive_summary("未打开压缩包".into());
                    app.set_status_text("无法打开".into());
                    app.set_status_detail(error.to_string().into());
                    app.set_has_error(true);
                }
            }
        });
    });
}

struct LoadedArchive {
    format: &'static str,
    browser: ArchiveBrowser,
    files: Vec<FileItem>,
    breadcrumbs: Vec<crate::BreadcrumbItem>,
    entry_count: usize,
    total: u64,
}

fn load_archive(path: &Path) -> Result<LoadedArchive, ArchiveError> {
    let format = detect_format(path)?;
    let mut archive = open_archive(path)?;
    let entries = archive.list()?;
    let entry_count = entries.len();
    let total = entries
        .iter()
        .filter(|entry| entry.is_file())
        .fold(0u64, |sum, entry| sum.saturating_add(entry.size));
    let browser = ArchiveBrowser::new(file_name(path), entries);
    let files = browser.items("");
    let breadcrumbs = browser.breadcrumbs("");
    Ok(LoadedArchive {
        format: format.label(),
        browser,
        files,
        breadcrumbs,
        entry_count,
        total,
    })
}

fn activate_item(app: &MainWindow, state: &SharedController, index: i32) {
    let directory = state.lock().ok().and_then(|state| {
        let browser = state.browser.as_ref()?;
        if index < 0 {
            return None;
        }
        browser.child_directory(&state.directory, index as usize)
    });
    if let Some(directory) = directory {
        show_directory(app, state, directory);
    }
}

fn show_directory(app: &MainWindow, state: &SharedController, directory: String) {
    let view = state.lock().ok().and_then(|mut state| {
        let browser = state.browser.as_ref()?;
        if !browser.contains_directory(&directory) {
            return None;
        }
        let files = browser.items(&directory);
        let breadcrumbs = browser.breadcrumbs(&directory);
        state.directory = directory;
        Some((files, breadcrumbs))
    });
    if let Some((files, breadcrumbs)) = view {
        app.set_files(ModelRc::new(VecModel::from(files)));
        app.set_breadcrumbs(ModelRc::new(VecModel::from(breadcrumbs)));
        app.set_selected(-1);
        app.set_can_navigate_up(
            !state
                .lock()
                .map(|state| state.directory.is_empty())
                .unwrap_or(true),
        );
        app.set_status_detail("".into());
    }
}

fn begin_extract(
    app: &MainWindow,
    state: &SharedController,
    archive: PathBuf,
    destination: PathBuf,
) {
    let cancel = Arc::new(AtomicBool::new(false));
    let task_id = {
        let Ok(mut controller) = state.lock() else {
            return;
        };
        controller.task_id += 1;
        controller.cancel = Some(cancel.clone());
        controller.close_requested = false;
        controller.task_id
    };
    app.set_busy(true);
    app.set_extracting(true);
    app.set_progress(0.0);
    app.set_progress_text("正在准备…".into());
    app.set_current_file("正在校验归档路径…".into());
    app.set_status_text("正在提取".into());
    app.set_status_detail(destination.display().to_string().into());
    app.set_has_error(false);

    let weak = app.as_weak();
    let worker_state = state.clone();
    let completed = Arc::new(AtomicUsize::new(0));
    let worker_completed = completed.clone();
    std::thread::spawn(move || {
        let mut reporter = ProgressReporter::new(weak.clone(), worker_state.clone(), task_id);
        let result = open_archive(&archive).and_then(|mut archive| {
            archive.extract(
                &destination,
                &mut |progress| {
                    completed.store(progress.completed, Ordering::Relaxed);
                    reporter.report(progress);
                },
                &cancel,
            )
        });
        post(&weak, move |app| {
            let close = match worker_state.lock() {
                Ok(mut controller) if controller.task_id == task_id => {
                    controller.cancel = None;
                    controller.close_requested
                }
                _ => return,
            };
            app.set_busy(false);
            app.set_extracting(false);
            if close {
                let _ = app.hide();
                return;
            }
            let count = worker_completed.load(Ordering::Relaxed);
            match result {
                Ok(report) => {
                    app.set_progress(1.0);
                    app.set_status_text("提取完成".into());
                    app.set_status_detail(
                        format!(
                            "已完成 {} 个文件，输出到 {}",
                            report.completed,
                            report.output_dir.display()
                        )
                        .into(),
                    );
                    app.set_has_error(false);
                }
                Err(error) => {
                    app.set_status_text(if error.is_cancelled() {
                        "已取消".into()
                    } else {
                        "提取失败".into()
                    });
                    app.set_status_detail(format!("{error}；此前已完成 {count} 个文件").into());
                    app.set_has_error(!error.is_cancelled());
                }
            }
        });
    });
}

struct ProgressReporter {
    weak: Weak<MainWindow>,
    state: SharedController,
    task_id: u64,
    last_sent: Instant,
    last_file: String,
}

impl ProgressReporter {
    fn new(weak: Weak<MainWindow>, state: SharedController, task_id: u64) -> Self {
        Self {
            weak,
            state,
            task_id,
            last_sent: Instant::now() - Duration::from_millis(100),
            last_file: String::new(),
        }
    }

    fn report(&mut self, progress: ExtractionProgress) {
        if progress.current != self.last_file
            || self.last_sent.elapsed() >= Duration::from_millis(50)
            || progress.processed == progress.total
        {
            self.last_sent = Instant::now();
            self.last_file = progress.current.clone();
            let state = self.state.clone();
            let task_id = self.task_id;
            post(&self.weak, move |app| {
                if state
                    .lock()
                    .map(|state| state.task_id != task_id)
                    .unwrap_or(true)
                {
                    return;
                }
                let fraction = if progress.total == 0 {
                    0.0
                } else {
                    (progress.processed as f64 / progress.total as f64).clamp(0.0, 1.0)
                };
                app.set_progress(fraction as f32);
                app.set_progress_text(
                    format!(
                        "{} / {} · {:.0}%",
                        format_size(progress.processed),
                        format_size(progress.total),
                        fraction * 100.0
                    )
                    .into(),
                );
                app.set_current_file(if progress.current.is_empty() {
                    "正在准备…".into()
                } else {
                    progress.current.into()
                });
            });
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn post<F>(weak: &Weak<MainWindow>, callback: F)
where
    F: FnOnce(MainWindow) + Send + 'static,
{
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = weak.upgrade() {
            callback(app);
        }
    });
}
