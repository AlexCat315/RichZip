//! Floating assistant menu for MugenZip.
//!
//! `mugenzip --widget` starts a small, always-on-top dot instead of the main
//! window. Mature desktop floating assistants (AssistiveTouch, SoftCircle,
//! 腾讯管家侠) are all a **two-layer** component, not a decorative sphere:
//!
//!   1. A solid, opaque dot that stays out of the way. It lifts on hover,
//!      accepts dragged archives, and can be hidden from the tray or a global
//!      hotkey.
//!   2. A radial menu that opens on click, with six icon-and-label actions
//!      arranged in a ring, hover feedback, and click-outside to dismiss.
//!
//! The window is a shaped ellipse in both states; expanding it resizes the
//! window (the native layer rebuilds the shape mask) while keeping the dot's
//! centre fixed, so the menu grows out of the dot instead of jumping.

use std::cell::{Cell, RefCell};
use std::f32::consts::PI;
use std::path::PathBuf;
use std::rc::Rc;

use cove_ui::ApplicationLaunch;
use cove_ui::compose::modifiers::style::style_modifiers::AppearanceExt;
use cove_ui::compose::modifiers::interaction::{AccessibilityExt, HitTestingExt, HoverExt};
use cove_ui::compose::modifiers::layout::LayoutFrameExt;
use cove_ui::compose::prelude::{SemanticFontRef, Text};
use cove_ui::compose::theme::tokens::semantic_modifiers::SemanticStyleViewExt;
use cove_ui::compose::primitives::interaction::{DroppedFile, FileDropZone};
use cove_ui::compose::primitives::media::ui_icon_catalog;
use cove_ui::compose::primitives::media::{Image, ImageSource};
use cove_ui::compose::theme::styles::buttons::button_style::ButtonStyleViewExt;
use cove_ui::compose::theme::styles::buttons::button_style::plain;
use cove_ui::compose::{AnyView, PrimitiveView, impl_primitive_view};
use cove_ui::composition::layout::{Layout, LayoutContainer, LayoutSubview, ProposedViewSize};
use cove_ui::composition::{DynamicField, Scope, TupleView, View, ViewExt};
use cove_ui::core::scene::{
    EventPhase, EventResult, HoverPhase, InputHandlerRoute, InteractionRegistryHolder,
    LayoutNode, Node, Point, PointerCaptureHolder, PointerPhase, Rect, Size,
};
use cove_ui::platform::MouseButton;
use cove_ui::core::{Color, DrawList, State};
use cove_ui::hosts::desktop::application::composition::AppDisplayHandleHolder;
use cove_ui::hosts::status_item::{self, StatusEntry};
use cove_ui::hosts::{AppDisplayHandle, HotKey, HotKeyModifiers, WindowShape};

use crate::{compress_service, extract_service};
use crate::model::ARCHIVE_EXTENSIONS;

/// Collapsed dot diameter (and window size).
const DOT: f32 = 56.0;
/// Expanded radial menu diameter (and window size).
const DISC: f32 = 300.0;
/// Distance from the menu centre to each action's centre.
const RING: f32 = 94.0;
/// Radius of each action's circular button.
const ITEM_RADIUS: f32 = 25.0;
/// Gap from the primary display edges when parking the dot.
const MARGIN: f32 = 26.0;
const HOTKEY_ID: i32 = 1;

const STATUS_MENU: &[StatusEntry] = &[
    StatusEntry {
        id: "open",
        title: "打开 MugenZip 主界面",
    },
    StatusEntry {
        id: "toggle",
        title: "显示 / 隐藏悬浮球",
    },
    StatusEntry { id: "", title: "" },
    StatusEntry {
        id: "quit",
        title: "退出悬浮球",
    },
];

/// What a ring action does. Kept as data so the same table drives drawing,
/// hit-testing and execution — adding an action touches exactly one place.
#[derive(Clone, Copy, Debug)]
enum Command {
    Home,
    Extract,
    Compress,
    Settings,
    Hide,
    Quit,
}

struct Entry {
    label: &'static str,
    icon: &'static str,
    command: Command,
}

const ENTRIES: [Entry; 6] = [
    Entry { label: "主界面", icon: "home",     command: Command::Home },
    Entry { label: "解压",   icon: "download", command: Command::Extract },
    Entry { label: "压缩",   icon: "package",  command: Command::Compress },
    Entry { label: "设置",   icon: "settings", command: Command::Settings },
    Entry { label: "隐藏",   icon: "minimize", command: Command::Hide },
    Entry { label: "退出",   icon: "log-out",  command: Command::Quit },
];

/// Centre of the `i`th ring action, relative to the window's top-left corner.
fn item_center(index: usize, size: f32) -> Point {
    let angle = -PI / 2.0 + (index as f32) * (2.0 * PI / ENTRIES.len() as f32);
    Point::new(
        size / 2.0 + RING * angle.cos(),
        size / 2.0 + RING * angle.sin(),
    )
}

/// Labels live **outside** each action well — above, below, right, left — so
/// they never crowd the centre close target or overlap a neighbouring well.
fn label_rect(index: usize, size: f32) -> (f32, f32, f32, f32) {
    let p = item_center(index, size);
    let w = 40.0;
    let h = 18.0;
    match index {
        0 => (p.x - w / 2.0, p.y - ITEM_RADIUS - 4.0 - h, w, h),
        3 => (p.x - w / 2.0, p.y + ITEM_RADIUS + 4.0, w, h),
        1 | 2 => (p.x + ITEM_RADIUS + 4.0, p.y - h / 2.0, w, h),
        _ => (p.x - ITEM_RADIUS - 4.0 - w, p.y - h / 2.0, w, h),
    }
}

/// One action's clickable region: the icon well plus its label.
fn action_rect(index: usize, size: f32) -> (f32, f32, f32, f32) {
    let p = item_center(index, size);
    let (lx, ly, lw, lh) = label_rect(index, size);
    let x0 = (p.x - 30.0).min(lx);
    let y0 = (p.y - 30.0).min(ly);
    let x1 = (p.x + 30.0).max(lx + lw);
    let y1 = (p.y + 30.0).max(ly + lh);
    (x0, y0, x1 - x0, y1 - y0)
}

/// Builds the floating-menu application declaration.
pub fn widget_application() -> ApplicationLaunch {
    crate::widget_host::record_widget_pid();
    let state = WidgetState::default();
    let root = MenuView { state: state.clone() };
    ApplicationLaunch::new(root).configure(|app| {
        app.diagnostics.dev_tools = None;
        app.window.title = "MugenZip 悬浮菜单".into();
        app.window.width = DOT as i32;
        app.window.height = DOT as i32;
        app.window.min_width = DOT as i32;
        app.window.min_height = DOT as i32;
        app.window.title_bar_style = cove_ui::hosts::desktop::application::configuration::AppWindowTitleBarStyle::Borderless;
        app.window.shows_dock_icon = false;
        app.window.shape = Some(WindowShape::Ellipse);
        app.window.rendering.clear_color = Some([0.0, 0.0, 0.0, 0.0]);
    })
}

#[derive(Clone)]
struct WidgetState {
    hooks_installed: Rc<Cell<bool>>,
    visible: Rc<Cell<bool>>,
    expanded: State<bool>,
    targeted: State<bool>,
    dot_hovered: State<bool>,
    /// Index of the hovered ring action, or -1.
    active: State<i32>,
    display: Rc<RefCell<Option<AppDisplayHandle>>>,
    drag: Rc<RefCell<DragSession>>,
}

/// Tracks a press on the dot so a short click expands the ring and a longer
/// move drags the whole shaped window.
#[derive(Default)]
struct DragSession {
    active: bool,
    moved: bool,
    origin: (f32, f32),
    pointer_origin: (f32, f32),
    offset: (f32, f32),
}

impl Default for WidgetState {
    fn default() -> Self {
        Self {
            hooks_installed: Rc::new(Cell::new(false)),
            visible: Rc::new(Cell::new(true)),
            expanded: State::new(false),
            targeted: State::new(false),
            dot_hovered: State::new(false),
            active: State::new(-1),
            display: Rc::new(RefCell::new(None)),
            drag: Rc::new(RefCell::new(DragSession::default())),
        }
    }
}

impl WidgetState {
    fn ensure_hooks(&self) {
        if self.hooks_installed.get() {
            return;
        }
        let Some(display) = AppDisplayHandleHolder::current() else {
            // Installed during runtime bootstrap, possibly after the first
            // composition pass. Do not mark installed so the next body retries.
            return;
        };
        self.hooks_installed.set(true);
        *self.display.borrow_mut() = Some(display.clone());
        // The dot is a resident helper: it must stay reachable above normal
        // windows and must never acquire a Dock tile.
        display.set_window_always_on_top(true);

        // Display queries can fail while SDL's video subsystem boots. Retry on
        // the UI run loop; in environments that never report displays the dot
        // simply keeps the window manager's default placement.
        park_at_top_right(&display, 0);

        let hotkey_state = self.clone();
        let hotkey_display = display.clone();
        let _ = display.register_hotkey(
            HotKey::new(
                HOTKEY_ID,
                HotKeyModifiers { control: true, shift: true, ..Default::default() },
                b'K' as i32,
            ),
            Rc::new(move || {
                // Shortcut behaviour is "call the menu up", not "toggle a
                // hidden flag": a hidden dot is shown already expanded, and a
                // visible dot opens or closes its ring.
                if !hotkey_state.visible.get() {
                    hotkey_state.visible.set(true);
                    hotkey_display.show_window();
                    hotkey_state.set_expanded(true);
                } else {
                    hotkey_state.set_expanded(!hotkey_state.expanded.get());
                }
            }),
        );

        let tray_state = self.clone();
        let tray_display = display.clone();
        let _ = status_item::install(
            &tray_icon(),
            "MugenZip 悬浮菜单",
            STATUS_MENU,
            move |id| match id {
                "open" => open_home(),
                "toggle" => toggle(&tray_display, &tray_state),
                "quit" => quit(&tray_display),
                _ => {}
            },
        );
    }

    /// Grow or shrink the shaped window while keeping the dot's centre fixed.
    fn set_expanded(&self, expanded: bool) {
        if self.expanded.get() == expanded {
            return;
        }
        let Some(display) = self.display.borrow().clone() else {
            return;
        };
        let old = if self.expanded.get() { DISC } else { DOT };
        let new = if expanded { DISC } else { DOT };

        let (mut x, mut y) = display.window_position().unwrap_or((0.0, 0.0));
        let cx = x + old / 2.0;
        let cy = y + old / 2.0;
        x = cx - new / 2.0;
        y = cy - new / 2.0;

        // Keep the disc on whichever monitor the dot actually occupies. The
        // old implementation clamped to display 0, which pushed a dot parked
        // on a secondary screen back to the primary one.
        if expanded {
            if let Some((bx, by, bw, bh)) = bounds_containing(&display, cx, cy) {
                x = x.clamp(
                    bx + MARGIN,
                    (bx + bw - DISC - MARGIN).max(bx + MARGIN),
                );
                y = y.clamp(
                    by + MARGIN,
                    (by + bh - DISC - MARGIN).max(by + MARGIN),
                );
            }
        }

        display.set_window_position(x, y);
        display.set_window_size(new, new);
        self.expanded.set(expanded);
        self.active.set(-1);
        display.request_display();
    }

    fn execute(&self, command: Command) {
        let display = self
            .display
            .try_borrow()
            .ok()
            .and_then(|display| display.clone());
        match command {
            Command::Home => open_home(),
            Command::Settings => open_settings(),
            Command::Extract => {
                if let Some(display) = display {
                    display.request_open_file(
                        &[],
                        true,
                        None,
                        Box::new(|paths| {
                            for path in paths {
                                extract_service::run(
                                    PathBuf::from(path),
                                    extract_service::Placement::IntoFolder,
                                );
                            }
                        }),
                    );
                }
            }
            Command::Compress => {
                if let Some(display) = display {
                    display.request_open_file(
                        &[],
                        true,
                        None,
                        Box::new(|paths| {
                            if paths.is_empty() {
                                return;
                            }
                            compress_service::run(
                                paths.into_iter().map(PathBuf::from).collect(),
                                mugenzip_core::CreateFormat::Zip,
                                mugenzip_core::CompressionLevel::Balanced,
                            );
                        }),
                    );
                }
            }
            Command::Hide => {
                if let Some(display) = display {
                    self.visible.set(false);
                    display.hide_window();
                }
            }
            Command::Quit => {
                if let Some(display) = display {
                    quit(&display);
                } else {
                    std::process::exit(0);
                }
                return;
            }
        }
        // Collapse last. Resizing the shaped window from inside the click
        // handler can rebuild the view before the action has been dispatched.
        self.set_expanded(false);
    }
}

fn park_at_top_right(display: &AppDisplayHandle, attempt: u32) {
    if let Some(bounds) = display
        .display_bounds(0, true)
        .or_else(|| display.display_bounds(0, false))
    {
        display.set_window_position(
            bounds.x + bounds.width - DOT - MARGIN,
            bounds.y + bounds.height * 0.35,
        );
        // A window resize or the initial show recentres the window on the
        // following frames, so re-apply a few times instead of once.
        if attempt >= 8 {
            return;
        }
        let display = display.clone();
        cove_ui::core::scene::UIWorkSchedulerHolder::schedule(Rc::new(move || {
            park_at_top_right(&display, attempt + 1);
        }));
        return;
    }
    if attempt >= 30 {
        return;
    }
    let display = display.clone();
    cove_ui::core::scene::UIWorkSchedulerHolder::schedule(Rc::new(move || {
        park_at_top_right(&display, attempt + 1);
    }));
}

fn toggle(display: &AppDisplayHandle, state: &WidgetState) {
    if state.visible.get() {
        // Park the dot before hiding: the tray and the hotkey both expect the
        // next show to start collapsed at the dot's old anchor.
        state.set_expanded(false);
        state.visible.set(false);
        display.hide_window();
    } else {
        state.visible.set(true);
        display.show_window();
    }
}

/// The monitor a point belongs to, preferring its usable area. Falls back to
/// the primary display so a window parked before displays are reported still
/// gets a sensible bound.
fn bounds_containing(display: &AppDisplayHandle, x: f32, y: f32) -> Option<(f32, f32, f32, f32)> {
    let mut fallback = None;
    for index in 0..display.display_count().max(1) {
        let bounds = display
            .display_bounds(index, true)
            .or_else(|| display.display_bounds(index, false))?;
        if fallback.is_none() {
            fallback = Some((bounds.x, bounds.y, bounds.width, bounds.height));
        }
        if x >= bounds.x
            && y >= bounds.y
            && x < bounds.x + bounds.width
            && y < bounds.y + bounds.height
        {
            return Some((bounds.x, bounds.y, bounds.width, bounds.height));
        }
    }
    fallback
}

fn is_archive(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ARCHIVE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// A drop of archives extracts each one beside itself; anything else — a file
/// or a folder, or a mix — becomes one ZIP. That is the only reading of a
/// generic drop that is useful without asking a second question.
fn handle_dropped(files: Vec<DroppedFile>) {
    if files.is_empty() {
        return;
    }
    let paths: Vec<PathBuf> = files.into_iter().map(|file| PathBuf::from(file.path)).collect();
    if paths.iter().all(|path| is_archive(path)) {
        for path in paths {
            extract_service::run(path, extract_service::Placement::IntoFolder);
        }
    } else {
        compress_service::run(
            paths,
            mugenzip_core::CreateFormat::Zip,
            mugenzip_core::CompressionLevel::Balanced,
        );
    }
}

fn open_home() {
    open_main(None);
}

fn open_settings() {
    open_main(Some("--settings"));
}

/// Brings the browser window forward, starting it when it is not running.
///
/// The dot is a separate bundle (`MugenZipWidget.app`), so the browser is a
/// distinct app rather than a second copy of the same bundle. That is what
/// makes the "already running" branch reliable: macOS cannot silently route
/// this request back into the dot itself.
fn open_main(fallback_arg: Option<&str>) {
    if let Some(pid) = main_process_id() {
        activate_main(pid, fallback_arg.is_some());
        return;
    }
    let executable = crate::widget_host::main_executable();
    let mut command = std::process::Command::new(executable);
    if let Some(arg) = fallback_arg {
        command.arg(arg);
    }
    let _ = command.spawn();
}

/// PID of a live browser instance, or `None` when only the dot is running.
fn main_process_id() -> Option<i32> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("ps")
            .args(["-axo", "pid=,command="])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let line = line.trim_start();
            let Some((pid, command)) = line.split_once(char::is_whitespace) else {
                continue;
            };
            let Ok(pid) = pid.parse::<i32>() else {
                continue;
            };
            if command.contains("MugenZip.app/Contents/MacOS/mugenzip")
                && !command.contains("--widget")
            {
                return Some(pid);
            }
        }
        None
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let self_path = std::env::current_exe().ok()?;
        for entry in std::fs::read_dir("/proc").ok()? {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            let mut parts = raw.split(|byte| *byte == 0);
            let Some(exe) = parts.next() else {
                continue;
            };
            let exe = std::path::PathBuf::from(String::from_utf8_lossy(exe).to_string());
            if exe != self_path {
                continue;
            }
            let has_widget = parts.any(|arg| arg == b"--widget");
            if !has_widget {
                return Some(pid);
            }
        }
        None
    }
    #[cfg(windows)]
    {
        crate::widget_host::running_main_pid().map(|pid| pid as i32)
    }
}

/// The tray icon: an SF Symbol on macOS, a real BMP path on the SDL tray
/// platforms. A missing asset degrades to the platform default instead of
/// dropping the tray item entirely.
fn tray_icon() -> String {
    #[cfg(target_os = "macos")]
    {
        "archivebox".to_owned()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut candidates = Vec::new();
        if let Some(dir) = std::env::var_os("MUGENZIP_TRAY_ICON") {
            candidates.push(PathBuf::from(dir));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("assets").join("tray.bmp"));
                candidates.push(dir.join("..").join("share").join("pixmaps").join("mugenzip-tray.bmp"));
            }
        }
        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join("tray.bmp"));
        for path in candidates {
            if path.is_file() {
                return path.to_string_lossy().into_owned();
            }
        }
        String::new()
    }
}

#[cfg(target_os = "macos")]
fn activate_main(pid: i32, settings: bool) {
    let mut script = format!(
        "tell application \"System Events\"\nset frontmost of process id {pid} to true",
    );
    if settings {
        script.push_str("\ndelay 0.2\nkeystroke \",\" using command down");
    }
    script.push_str("\nend tell");
    let _ = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status();
}

#[cfg(windows)]
fn activate_main(pid: i32, _settings: bool) {
    use std::ffi::c_void;
    type Hwnd = *mut c_void;
    const SW_RESTORE: i32 = 9;
    unsafe extern "system" {
        fn EnumWindows(
            callback: unsafe extern "system" fn(Hwnd, isize) -> i32,
            lparam: isize,
        ) -> i32;
        fn GetWindowThreadProcessId(window: Hwnd, pid: *mut u32) -> u32;
        fn IsWindowVisible(window: Hwnd) -> i32;
        fn ShowWindow(window: Hwnd, command: i32) -> i32;
        fn SetForegroundWindow(window: Hwnd) -> i32;
    }
    unsafe extern "system" fn find(window: Hwnd, lparam: isize) -> i32 {
        let target = unsafe { &mut *(lparam as *mut (u32, Option<Hwnd>)) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, &mut pid) };
        if pid == target.0 && unsafe { IsWindowVisible(window) } != 0 {
            target.1 = Some(window);
            return 0;
        }
        1
    }
    let mut target = (pid as u32, None);
    unsafe {
        EnumWindows(find, &mut target as *mut _ as isize);
    }
    if let Some(window) = target.1 {
        unsafe {
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn activate_main(_pid: i32, _settings: bool) {
    // X11 sessions usually have one of these. On Wayland there is no portable
    // way to raise another client, so a missing tool is a quiet no-op.
    for (program, args) in [
        ("xdotool", vec!["search", "--name", "MugenZip", "windowactivate"]),
        ("wmctrl", vec!["-a", "MugenZip"]),
    ] {
        if std::process::Command::new(program)
            .args(args)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        {
            break;
        }
    }
}

fn quit(display: &AppDisplayHandle) {
    crate::widget_host::clear_widget_pid();
    // Let the runtime unhook the global hotkey, remove the status item and
    // tear down the shaped window instead of killing the process.
    display.quit();
}

// ---------------------------------------------------------------------------
// View composition
// ---------------------------------------------------------------------------

/// The collapsed dot. It owns pointer capture so a drag keeps following the
/// cursor even after the window has moved out from under it.
struct DragDot {
    state: WidgetState,
}

impl PrimitiveView for DragDot {
    fn make_node(&self) -> Node {
        let node = Node::new();
        node.set_is_hit_testable(true);
        node
    }

    fn update_node(&self, node: &Node) {
        let Some(registry) = InteractionRegistryHolder::current() else {
            return;
        };
        let drag_state = self.state.clone();
        registry.set_hover(
            node,
            Rc::new(move |phase| {
                // Rebuilding the view mid-drag would replace the node that owns
                // pointer capture, so hover only updates while no press is live.
                if !drag_state.drag.borrow().active {
                    drag_state.dot_hovered.set(phase == HoverPhase::Enter);
                }
            }),
        );

        let pointer_state = self.state.clone();
        let pointer_node = node.downgrade();
        registry.set_pointer(
            node,
            InputHandlerRoute::control(),
            Rc::new(move |event, phase, event_phase| {
                if event_phase == EventPhase::Capture || event.button != MouseButton::Left {
                    return EventResult::Ignored;
                }
                match phase {
                    PointerPhase::Down => {
                        let Some(display) = pointer_state
                            .display
                            .try_borrow()
                            .ok()
                            .and_then(|display| display.clone())
                        else {
                            return EventResult::Ignored;
                        };
                        let origin = display.window_position().unwrap_or((0.0, 0.0));
                        let pointer_origin = display.global_pointer().unwrap_or(origin);
                        *pointer_state.drag.borrow_mut() = DragSession {
                            active: true,
                            moved: false,
                            origin,
                            pointer_origin,
                            offset: (0.0, 0.0),
                        };
                        if let Some(node) = Node::upgrade(&pointer_node)
                            && let Some(capture) = PointerCaptureHolder::current()
                        {
                            capture.acquire(&node, true);
                        }
                        EventResult::Handled
                    }
                    PointerPhase::Up => {
                        let (active, moved, origin, pointer_origin) = {
                            let mut drag = pointer_state.drag.borrow_mut();
                            let snapshot = (drag.active, drag.moved, drag.origin, drag.pointer_origin);
                            drag.active = false;
                            snapshot
                        };
                        if let Some(node) = Node::upgrade(&pointer_node)
                            && let Some(capture) = PointerCaptureHolder::current()
                            && capture.target().as_ref() == Some(&node)
                        {
                            capture.release();
                        }
                        if !active {
                            return EventResult::Ignored;
                        }
                        if moved
                            && let Some(display) = pointer_state
                                .display
                                .try_borrow()
                                .ok()
                                .and_then(|display| display.clone())
                            && let Some(pointer) = display.global_pointer()
                        {
                            display.set_window_position(
                                origin.0 + pointer.0 - pointer_origin.0,
                                origin.1 + pointer.1 - pointer_origin.1,
                            );
                        }
                        if !moved {
                            pointer_state.set_expanded(!pointer_state.expanded.get());
                        }
                        EventResult::Handled
                    }
                }
            }),
        );

        let motion_state = self.state.clone();
        registry.set_motion(
            node,
            InputHandlerRoute::control(),
            Rc::new(move |event, phase| {
                if phase == EventPhase::Capture {
                    return EventResult::Ignored;
                }
                let mut drag = motion_state.drag.borrow_mut();
                if !drag.active {
                    return EventResult::Ignored;
                }
                let display = motion_state
                    .display
                    .try_borrow()
                    .ok()
                    .and_then(|display| display.clone());
                let pointer = display.as_ref().and_then(|display| display.global_pointer());
                if let Some(pointer) = pointer {
                    drag.offset = (
                        pointer.0 - drag.pointer_origin.0,
                        pointer.1 - drag.pointer_origin.1,
                    );
                } else {
                    drag.offset.0 += event.delta_x;
                    drag.offset.1 += event.delta_y;
                }
                if drag.offset.0.abs() + drag.offset.1.abs() > 4.0 {
                    drag.moved = true;
                }
                let target = (drag.origin.0 + drag.offset.0, drag.origin.1 + drag.offset.1);
                drop(drag);
                if let Some(display) = display {
                    display.set_window_position(target.0, target.1);
                }
                EventResult::Handled
            }),
        );
    }
}

impl_primitive_view!(DragDot);

struct MenuView {
    state: WidgetState,
}

impl View for MenuView {
    fn body(&self, _cx: &Scope) -> AnyView {
        self.state.ensure_hooks();

        let expanded = self.state.expanded.get();
        let targeted = self.state.targeted.get();
        let dot_hovered = self.state.dot_hovered.get();
        let active = self.state.active.get();
        let size = if expanded { DISC } else { DOT };

        let canvas = AnyView::new(
            AnyView::new(MenuCanvas::new(expanded, targeted, dot_hovered, active))
                .frame_size(size, size),
        );

        // Collapsed dot: press-and-drag moves the shaped window, a plain click
        // expands the ring. Replacing the old full-window Button avoids the
        // click-vs-drag ambiguity that made the dot feel nailed down.
        let dot = AnyView::new(DragDot { state: self.state.clone() })
            .frame_size(size, size)
            .erased();

        // Collapsed, the whole dot is one button. Expanded, that full-window
        // button would swallow every ring action, so slot 1 becomes a
        // non-interactive placeholder and the centre X is the dismiss target.
        let mut layers: Vec<AnyView> = vec![canvas];
        if expanded {
            // A laid-out, non-interactive placeholder keeps the radial
            // index contract (0/1 full-bleed, 2 centre, 3..8 ring, 9..14
            // labels). EmptyView is optimised away and shifts every hit.
            layers.push(
                Text::new(" ")
                    .frame_size(size, size)
                    .allows_hit_testing(false)
                    .accessibility_hidden(true)
                    .erased(),
            );

            // Centre close target.
            let close_state = self.state.clone();
            layers.push(
                AnyView::new(
                    cove_ui::compose::primitives::controls::Button::new(
                        cove_ui::composition::EmptyView {},
                        move || close_state.set_expanded(false),
                    )
                    .button_style(plain()),
                )
                .frame_size(60.0, 60.0)
                .erased(),
            );

            // Six hit targets on the ring (transparent — the icon and its
            // circular well are drawn by the canvas), then six text labels.
            for (index, entry) in ENTRIES.iter().enumerate() {
                let hit_state = self.state.clone();
                let command = entry.command;
                let hover_state = self.state.clone();
                let (_, _, width, height) = action_rect(index, DISC);
                layers.push(
                    AnyView::new(
                        cove_ui::compose::primitives::controls::Button::new(
                            cove_ui::composition::EmptyView {},
                            move || hit_state.execute(command),
                        )
                        .button_style(plain()),
                    )
                    .frame_size(width, height)
                    .on_hover(move |entering| {
                        hover_state.active.set(if entering { index as i32 } else { -1 });
                    })
                    .erased(),
                );
            }
            for entry in ENTRIES.iter() {
                let label = Text::new(entry.label)
                    .alignment(cove_ui::text::TextAlignment::Center)
                    .semantic_font(SemanticFontRef::caption())
                    .foreground_color(Color { a: 0.86, ..Color::WHITE });
                layers.push(label.frame_size(40.0, 18.0).erased());
            }
        } else {
            layers.push(dot);
        }

        // A custom layout is required here: ZStack centres every child, which
        // would stack all six actions in the middle. This one places each layer
        // at its ring coordinate so hit-testing follows the visible icons.
        let stack = AnyView::new(LayoutContainer::<RadialLayout, AnyView>::with_erased_content(
            RadialLayout { size, expanded },
            AnyView::new(TupleView::new(layers)),
        ))
        .frame_size(size, size);

        let targeted_flag = self.state.clone();
        let drop = FileDropZone::new(handle_dropped, stack)
            // Archives extract; ordinary files and folders compress. Restricting
            // the zone to archive extensions made the "compress" side of the dot
            // unreachable by drag, which is half of its purpose.
            .allows_directories(true)
            .maximum_files(16)
            .shows_chrome(false)
            .is_targeted(self.state.targeted.binding())
            .on_targeted(move |value| targeted_flag.targeted.set(value))
            .frame_size(size, size);

        AnyView::new(drop)
    }

    fn dynamic_properties(&self) -> Vec<DynamicField> {
        vec![
            DynamicField::state("menuExpanded", &self.state.expanded),
            DynamicField::state("menuTargeted", &self.state.targeted),
            DynamicField::state("dotHovered", &self.state.dot_hovered),
            DynamicField::state("menuActive", &self.state.active),
        ]
    }
}

/// Places the layers of the floating menu. Indices are fixed by construction:
/// 0 canvas, 1 backdrop, 2 close target, 3..9 ring hit targets, 9..15 labels.
#[derive(Clone, Copy)]
struct RadialLayout {
    size: f32,
    expanded: bool,
}

impl Layout for RadialLayout {
    fn size_that_fits(&self, _proposal: ProposedViewSize, _subviews: &[LayoutSubview]) -> Size {
        Size { width: self.size, height: self.size }
    }

    fn place_subviews(&self, bounds: Rect, _proposal: ProposedViewSize, subviews: &[LayoutSubview]) {
        let full = |subview: &LayoutSubview| {
            subview.place(
                Point::new(bounds.x, bounds.y),
                ProposedViewSize::new(Some(self.size), Some(self.size)),
                true,
            );
        };
        for (index, subview) in subviews.iter().enumerate() {
            let (x, y, w, h) = match index {
                0 | 1 => {
                    full(subview);
                    continue;
                }
                2 if self.expanded => (self.size / 2.0 - 30.0, self.size / 2.0 - 30.0, 60.0, 60.0),
                i if self.expanded && (3..9).contains(&i) => action_rect(i - 3, self.size),
                i if self.expanded && (9..15).contains(&i) => label_rect(i - 9, self.size),
                _ => (0.0, 0.0, 0.0, 0.0),
            };
            subview.place(
                Point::new(bounds.x + x, bounds.y + y),
                ProposedViewSize::new(Some(w), Some(h)),
                true,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Retained canvas painting both states. Everything (backdrop, ring buttons,
/// icons) is one draw list so it composites correctly on a premultiplied-alpha
/// shaped window; only the hit targets are real views.
#[derive(Clone)]
struct MenuCanvas {
    expanded: bool,
    targeted: bool,
    dot_hovered: bool,
    active: i32,
    icons: Rc<RefCell<Vec<Option<ImageSource>>>>,
}

impl MenuCanvas {
    fn new(expanded: bool, targeted: bool, dot_hovered: bool, active: i32) -> Self {
        Self {
            expanded,
            targeted,
            dot_hovered,
            active,
            icons: Rc::new(RefCell::new(vec![None; ENTRIES.len() + 2])),
        }
    }

    fn size(&self) -> f32 {
        if self.expanded { DISC } else { DOT }
    }

    fn paint(&self, list: &mut DrawList, origin: Point) {
        if self.expanded {
            self.paint_disc(list, origin);
        } else {
            self.paint_dot(list, origin);
        }
    }

    fn paint_dot(&self, list: &mut DrawList, origin: Point) {
        let cx = origin.x + DOT / 2.0;
        let cy = origin.y + DOT / 2.0;
        let center = Point::new(cx, cy);
        let r = DOT / 2.0 - 2.5;
        let lit = self.targeted || self.dot_hovered;

        // Solid, not translucent: a floating control that lets terminal text
        // bleed through reads like a chat avatar, not an archive tool.
        let body = if self.targeted {
            rgb(0x2E, 0xC8, 0x86)
        } else if lit {
            rgb(0x22, 0x30, 0x2B)
        } else {
            rgb(0x15, 0x1E, 0x1B)
        };
        let _ = list.add_disk_shaded(center, r, move |_x, _y| {
            Color { a: 1.0, ..body }
        });
        // A crisp rim is what keeps a small circle from reading as a flat
        // UI swatch; the body itself stays one clean, solid colour.
        let _ = list.add_shaded_annulus(center, r - 1.0, r, |_x, _y| {
            Color { a: if lit { 0.58 } else { 0.32 }, ..Color::WHITE }
        });

        self.draw_icon(list, ENTRIES.len(), "archive", cx, cy, 20.0, 0.98);
    }

    fn paint_disc(&self, list: &mut DrawList, origin: Point) {
        let cx = origin.x + DISC / 2.0;
        let cy = origin.y + DISC / 2.0;
        let center = Point::new(cx, cy);
        let r = DISC / 2.0 - 6.0;

        // Contact shadow sits just inside the silhouette, then the disc itself
        // leaves the masked corners fully transparent.
        let _ = list.add_disk_shaded(center, r + 6.0, |_x, _y| {
            Color { r: 0.0, g: 0.0, b: 0.0, a: 0.18 }
        });
        let _ = list.add_disk_shaded(center, r, move |_x, y| {
            let t = ((y - cy) / r * 0.5 + 0.5).clamp(0.0, 1.0);
            Color {
                r: 0.050 + 0.022 * (1.0 - t),
                g: 0.070 + 0.030 * (1.0 - t),
                b: 0.064 + 0.028 * (1.0 - t),
                a: 1.0,
            }
        });
        let _ = list.add_shaded_annulus(center, r - 1.5, r, |_x, _y| {
            Color { a: 0.22, ..Color::WHITE }
        });
        // Centre: a quiet recessed well holding the close glyph.
        let well = 30.0;
        let _ = list.add_disk_shaded(center, well, |_x, _y| {
            Color { r: 1.0, g: 1.0, b: 1.0, a: 0.065 }
        });
        let _ = list.add_shaded_annulus(center, well - 1.0, well, |_x, _y| {
            Color { a: 0.10, ..Color::WHITE }
        });
        self.draw_icon(list, ENTRIES.len() + 1, "x", cx, cy, 19.0, 0.70);

        // Ring actions.
        for (index, entry) in ENTRIES.iter().enumerate() {
            let p = item_center(index, DISC);
            let pcx = origin.x + p.x;
            let pcy = origin.y + p.y;
            let pc = Point::new(pcx, pcy);
            let selected = self.active == index as i32;

            // Selected actions get an emerald fill; idle ones a faint well.
            let fill = if selected { rgb(0x2E, 0xB8, 0x7E) } else { Color::WHITE };
            let fill_alpha = if selected { 0.94 } else { 0.095 };
            let _ = list.add_disk_shaded(pc, ITEM_RADIUS, move |_x, _y| {
                Color { a: fill_alpha, ..fill }
            });
            let _ = list.add_shaded_annulus(pc, ITEM_RADIUS - 1.0, ITEM_RADIUS, |_x, _y| {
                Color { a: if selected { 0.60 } else { 0.13 }, ..Color::WHITE }
            });
            self.draw_icon(list, index, entry.icon, pcx, pcy, 21.0, if selected { 0.98 } else { 0.90 });
        }
    }

    fn draw_icon(
        &self,
        list: &mut DrawList,
        slot: usize,
        slug: &str,
        cx: f32,
        cy: f32,
        size: f32,
        alpha: f32,
    ) {
        let mut icons = self.icons.borrow_mut();
        if icons[slot].is_none() {
            let resource = ui_icon_catalog::asset(slug);
            icons[slot] = Some(Image::resolve_resource(&resource, 96.0, 96.0));
        }
        let Some(source) = icons[slot].clone() else {
            return;
        };
        // A dark drop shadow lifts the white glyph off the frosted surface.
        let shadow = cove_ui::core::Rect::new(cx - size / 2.0 + 1.0, cy - size / 2.0 + 1.0, size, size);
        let glyph = cove_ui::core::Rect::new(cx - size / 2.0, cy - size / 2.0, size, size);
        source.retain(list);
        let tex = source.texture_id();
        let uv0 = Point::new(0.0, 0.0);
        let uv1 = Point::new(1.0, 1.0);
        let _ = list.add_image_mask_quad(shadow, tex, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.35 }, uv0, uv1);
        let _ = list.add_image_mask_quad(glyph, tex, Color { a: alpha, ..Color::WHITE }, uv0, uv1);
    }
}

impl PrimitiveView for MenuCanvas {
    fn make_node(&self) -> Node {
        let node = Node::new();
        node.set_is_hit_testable(false);
        node
    }

    fn make_layout_node(&self) -> Option<LayoutNode> {
        let size = self.size();
        let layout = LayoutNode::new();
        layout.set_width(Some(size));
        layout.set_height(Some(size));
        Some(layout)
    }

    fn update_layout(&self, layout: &LayoutNode) {
        let size = self.size();
        layout.set_width(Some(size));
        layout.set_height(Some(size));
    }

    fn update_node(&self, node: &Node) {
        // Resolve all bundled SVGs once; they stay cached for the canvas' life.
        {
            let mut icons = self.icons.borrow_mut();
            for (index, entry) in ENTRIES.iter().enumerate() {
                if icons[index].is_none() {
                    let resource = ui_icon_catalog::asset(entry.icon);
                    icons[index] = Some(Image::resolve_resource(&resource, 96.0, 96.0));
                }
            }
            if icons[ENTRIES.len()].is_none() {
                let resource = ui_icon_catalog::asset("archive");
                icons[ENTRIES.len()] = Some(Image::resolve_resource(&resource, 96.0, 96.0));
            }
            if icons[ENTRIES.len() + 1].is_none() {
                let resource = ui_icon_catalog::asset("x");
                icons[ENTRIES.len() + 1] = Some(Image::resolve_resource(&resource, 96.0, 96.0));
            }
        }
        let identity = (self.expanded, self.targeted, self.dot_hovered, self.active);
        let canvas = self.clone();
        node.update_draw(identity, move |list, origin| {
            canvas.paint(list, origin);
        });
    }
}

impl_primitive_view!(MenuCanvas);

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 0xFF)
}
