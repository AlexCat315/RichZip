//! Floating-ball mode for MugenZip.
//!
//! `mugenzip --widget` starts a small, always-on-top circular window instead of
//! the browser. Drop an archive onto the ball and it is extracted headlessly
//! into a sibling folder — the same path the shell "extract here" verb takes —
//! so no browsing window ever appears. Clicking the ball opens the full app,
//! and a tray menu plus a global hotkey bring it back when hidden.
//!
//! This is a demonstration of Cove's floating-widget primitives (shaped window,
//! no dock icon, system tray, global hotkey) assembled into one small product
//! surface. It is deliberately self-contained: it reuses `extract_service` for
//! the actual work and never opens an archive in a visible window.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use cove_ui::ApplicationLaunch;
use cove_ui::compose::modifiers::layout::LayoutFrameExt;
use cove_ui::compose::modifiers::style::AppearanceExt;
use cove_ui::compose::primitives::controls::Button;
use cove_ui::compose::primitives::interaction::{DroppedFile, FileDropZone};
use cove_ui::compose::primitives::media::ui_icon_catalog;
use cove_ui::composition::{AnyView, DynamicField, Scope, View, views};
use cove_ui::core::State;
use cove_ui::hosts::desktop::application::composition::AppDisplayHandleHolder;
use cove_ui::hosts::status_item::{self, StatusEntry};
use cove_ui::hosts::{HotKey, HotKeyModifiers, WindowShape};

use crate::extract_service;
use crate::model::ARCHIVE_EXTENSIONS;

/// Diameter of the ball in logical points. A square window with an elliptical
/// silhouette is a circle, so the window is sized `BALL x BALL`.
const BALL: f32 = 132.0;
/// Gap from the primary display edges when parking the ball at the top-right.
const MARGIN: f32 = 28.0;
/// Application-chosen id for the toggle hotkey.
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

/// Builds the floating-ball application declaration.
pub fn widget_application() -> ApplicationLaunch {
    let state = WidgetState::default();
    let root = BallView {
        state: state.clone(),
    };
    ApplicationLaunch::new(root).configure(|app| {
        // A resident tray tool does not need a debugging server.
        app.diagnostics.dev_tools = None;
        app.window.title = "MugenZip 悬浮球".into();
        app.window.width = BALL as i32;
        app.window.height = BALL as i32;
        app.window.min_width = BALL as i32;
        app.window.min_height = BALL as i32;
        app.window.title_bar_style = cove_ui::hosts::desktop::application::configuration::AppWindowTitleBarStyle::HiddenInset;
        // No taskbar / Dock tile: the tray item is the way back.
        app.window.shows_dock_icon = false;
        // A circle inscribed in the square window; pixels outside the ellipse
        // are transparent and click-through on shaping backends.
        app.window.shape = Some(WindowShape::Ellipse);
        // Transparent background so only the drawn disc is visible.
        app.window.rendering.clear_color = Some([0.0, 0.0, 0.0, 0.0]);
    })
}

/// Shared widget state. Hooks are installed once, on the first body, which is
/// the earliest point the host ambients (display handle) are available.
#[derive(Clone)]
struct WidgetState {
    hooks_installed: Rc<Cell<bool>>,
    visible: Rc<Cell<bool>>,
    targeted: State<bool>,
    display: Rc<RefCell<Option<cove_ui::hosts::AppDisplayHandle>>>,
}

impl Default for WidgetState {
    fn default() -> Self {
        Self {
            hooks_installed: Rc::new(Cell::new(false)),
            visible: Rc::new(Cell::new(true)),
            targeted: State::new(false),
            display: Rc::new(RefCell::new(None)),
        }
    }
}

impl WidgetState {
    fn ensure_hooks(&self) {
        if self.hooks_installed.replace(true) {
            return;
        }
        let Some(display) = AppDisplayHandleHolder::current() else {
            return;
        };
        *self.display.borrow_mut() = Some(display.clone());

        // Park at the top-right of the primary display, using its usable area
        // so the ball clears the taskbar / menu bar.
        if let Some(bounds) = display.display_bounds(0, true) {
            display.set_window_position(
                bounds.x + bounds.width - BALL - MARGIN,
                bounds.y + MARGIN,
            );
        }

        // Ctrl+Shift+K toggles the ball. The runtime delivers the callback on
        // the UI thread, so it is safe to drive the window from here.
        let hotkey_display = display.clone();
        let hotkey_visible = self.visible.clone();
        let _ = display.register_hotkey(
            HotKey::new(
                HOTKEY_ID,
                HotKeyModifiers {
                    control: true,
                    shift: true,
                    ..Default::default()
                },
                b'K' as i32,
            ),
            Rc::new(move || toggle(&hotkey_display, &hotkey_visible)),
        );

        // Tray menu: open the browser, toggle the ball, or quit. Picks are
        // delivered on the UI thread that called `install` (this body).
        let tray_visible = self.visible.clone();
        let tray_display = display.clone();
        let _ = status_item::install("", "MugenZip 悬浮球", STATUS_MENU, move |id| match id {
            "open" => launch_browser(),
            "toggle" => toggle(&tray_display, &tray_visible),
            "quit" => std::process::exit(0),
            _ => {}
        });
    }
}

/// Flips visibility and moves the window accordingly. `Cell::replace` returns
/// the previous value: true means it was visible and is now being hidden.
fn toggle(display: &cove_ui::hosts::AppDisplayHandle, visible: &Rc<Cell<bool>>) {
    if visible.replace(!visible.get()) {
        display.hide_window();
    } else {
        display.show_window();
    }
}

/// Starts the normal MugenZip browser in a separate process (no `--widget`).
fn launch_browser() {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let _ = std::process::Command::new(executable).spawn();
}

/// Extracts every dropped archive in place, without showing a window. This is
/// exactly the shell verb's headless path, reused for drag-and-drop.
fn extract_dropped(files: Vec<DroppedFile>) {
    for file in files {
        if file.is_directory {
            continue;
        }
        extract_service::run(PathBuf::from(file.path), extract_service::Placement::IntoFolder);
    }
}

struct BallView {
    state: WidgetState,
}

impl View for BallView {
    fn body(&self, _cx: &Scope) -> AnyView {
        self.state.ensure_hooks();

        let targeted = self.state.targeted.get();
        // Deep pine when idle; a brighter jade ring while a file hovers.
        let disc = if targeted {
            cove_ui::core::Color::from_rgba8(0x2F, 0xA0, 0x6E, 0xFF)
        } else {
            cove_ui::core::Color::from_rgba8(0x1E, 0x6B, 0x4C, 0xFF)
        };
        let ring = if targeted {
            cove_ui::core::Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF)
        } else {
            cove_ui::core::Color::from_rgba8(0x8E, 0xD9, 0xB4, 0xFF)
        };
        // The archive glyph and caption use stock buttons: their label path is
        // known to draw in a shaped transparent window, where bare Icon/Text
        // views do not. Both open the full browser when clicked.
        let icon_button = cove_ui::compose::primitives::controls::Button::icon(
            cove_ui::compose::primitives::controls::ButtonIconSource::resource(
                ui_icon_catalog::file::archive(),
            ),
            52.0,
            launch_browser,
        );
        let caption = cove_ui::compose::primitives::controls::Button::title(
            "拖入解压",
            launch_browser,
        );
        let face = AnyView::new(
            cove_ui::composition::Column::new(
                cove_ui::composition::HorizontalAlignment::Center,
                2.0,
                AnyView::new(views!(icon_button, caption)),
            )
            .frame_size(BALL, BALL),
        );

        let targeted_state = self.state.clone();
        // The drop zone paints the circular disc. The face (icon + caption) is
        // layered on top in a ZStack: putting buttons inside the clipped zone
        // makes their labels disappear in a shaped transparent window.
        let zone = FileDropZone::new(extract_dropped, cove_ui::composition::EmptyView {})
            .accepted_extensions(ARCHIVE_EXTENSIONS)
            .maximum_files(16)
            .shows_chrome(false)
            .is_targeted(self.state.targeted.binding())
            .on_targeted(move |value| targeted_state.targeted.set(value))
            .frame_size(BALL, BALL)
            .corner_radius(BALL / 2.0)
            .background_color(disc)
            .border_color(ring, 2.0)
            .clipped();

        AnyView::new(
            cove_ui::compose::primitives::layout::ZStack::new(AnyView::new(views!(zone, face)))
                .frame_size(BALL, BALL),
        )
    }

    fn dynamic_properties(&self) -> Vec<DynamicField> {
        vec![DynamicField::state("ballTargeted", &self.state.targeted)]
    }
}
