//! Floating assistant menu for MugenZip.
//!
//! `mugenzip --widget` starts a small, always-on-top dot instead of the main
//! window. Mature desktop floating assistants (AssistiveTouch, SoftCircle,
//! 腾讯管家侠) are all a **two-layer** component, not a decorative sphere:
//!
//!   1. A quiet, translucent dot that stays out of the way. It fades when idle,
//!      brightens on hover, accepts dragged archives, and can be hidden from
//!      the tray or a global hotkey.
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
use cove_ui::compose::modifiers::interaction::HoverExt;
use cove_ui::compose::modifiers::layout::LayoutFrameExt;
use cove_ui::compose::primitives::interaction::{DroppedFile, FileDropZone};
use cove_ui::compose::primitives::media::ui_icon_catalog;
use cove_ui::compose::primitives::media::{Image, ImageSource};
use cove_ui::compose::theme::styles::buttons::button_style::ButtonStyleViewExt;
use cove_ui::compose::theme::styles::buttons::button_style::plain;
use cove_ui::compose::{AnyView, PrimitiveView, impl_primitive_view};
use cove_ui::composition::layout::{Layout, LayoutContainer, LayoutSubview, ProposedViewSize};
use cove_ui::composition::{DynamicField, Scope, TupleView, View};
use cove_ui::core::scene::{LayoutNode, Node, Point, Rect, Size};
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
#[derive(Clone, Copy)]
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

/// Builds the floating-menu application declaration.
pub fn widget_application() -> ApplicationLaunch {
    let state = WidgetState::default();
    let root = MenuView { state: state.clone() };
    ApplicationLaunch::new(root).configure(|app| {
        app.diagnostics.dev_tools = None;
        app.window.title = "MugenZip 悬浮菜单".into();
        app.window.width = DOT as i32;
        app.window.height = DOT as i32;
        app.window.min_width = DOT as i32;
        app.window.min_height = DOT as i32;
        app.window.title_bar_style = cove_ui::hosts::desktop::application::configuration::AppWindowTitleBarStyle::HiddenInset;
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

        // Display queries can fail while SDL's video subsystem boots. Retry on
        // the UI run loop; in environments that never report displays the dot
        // simply keeps the window manager's default placement.
        park_at_top_right(&display, 0);

        let hotkey_display = display.clone();
        let hotkey_visible = self.visible.clone();
        let hotkey_state = self.clone();
        let _ = display.register_hotkey(
            HotKey::new(
                HOTKEY_ID,
                HotKeyModifiers { control: true, shift: true, ..Default::default() },
                b'K' as i32,
            ),
            Rc::new(move || {
                // A hidden dot must be shown before it can expand.
                hotkey_state.visible.set(true);
                hotkey_display.show_window();
                toggle(&hotkey_display, &hotkey_visible);
            }),
        );

        let tray_visible = self.visible.clone();
        let tray_display = display.clone();
        let _ = status_item::install("", "MugenZip 悬浮菜单", STATUS_MENU, move |id| match id {
            "open" => launch_browser(),
            "toggle" => toggle(&tray_display, &tray_visible),
            "quit" => std::process::exit(0),
            _ => {}
        });
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

        // Keep the expanded disc on screen when the dot is parked at an edge.
        if expanded {
            if let Some(bounds) = display
                .display_bounds(0, true)
                .or_else(|| display.display_bounds(0, false))
            {
                x = x.clamp(bounds.x + MARGIN, bounds.x + bounds.width - DISC - MARGIN);
                y = y.clamp(bounds.y + MARGIN, bounds.y + bounds.height - DISC - MARGIN);
            }
        }

        display.set_window_position(x, y);
        display.set_window_size(new, new);
        self.expanded.set(expanded);
        self.active.set(-1);
        display.request_display();
    }

    fn execute(&self, command: Command) {
        self.set_expanded(false);
        let display = self.display.borrow().clone();
        match command {
            Command::Home | Command::Settings => launch_browser(),
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
            Command::Quit => std::process::exit(0),
        }
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

fn toggle(display: &AppDisplayHandle, visible: &Rc<Cell<bool>>) {
    if visible.replace(!visible.get()) {
        display.hide_window();
    } else {
        display.show_window();
    }
}

fn launch_browser() {
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let _ = std::process::Command::new(executable).spawn();
}

fn extract_dropped(files: Vec<DroppedFile>) {
    for file in files {
        if file.is_directory {
            continue;
        }
        extract_service::run(PathBuf::from(file.path), extract_service::Placement::IntoFolder);
    }
}

// ---------------------------------------------------------------------------
// View composition
// ---------------------------------------------------------------------------

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

        // Bottom layer: clicking the dot expands; clicking the disc's empty
        // space dismisses. Hover here drives the idle-fade of the dot.
        let toggle_state = self.state.clone();
        let hover_state = self.state.clone();
        let backdrop = AnyView::new(
            AnyView::new(
                cove_ui::compose::primitives::controls::Button::new(
                    cove_ui::composition::EmptyView {},
                    move || toggle_state.set_expanded(!toggle_state.expanded.get()),
                )
                .button_style(plain()),
            )
            .frame_size(size, size)
            .on_hover(move |entering| hover_state.dot_hovered.set(entering)),
        );

        let mut layers: Vec<AnyView> = vec![canvas, backdrop];

        if expanded {
            // Centre close target.
            let close_state = self.state.clone();
            layers.push(AnyView::new(
                cove_ui::compose::primitives::controls::Button::new(
                    cove_ui::composition::EmptyView {},
                    move || close_state.set_expanded(false),
                )
                .button_style(plain()),
            ));

            // Six hit targets on the ring (transparent — the icon and its
            // circular well are drawn by the canvas), then six text labels
            // sitting just below each icon so they never cover the glyph.
            for (index, entry) in ENTRIES.iter().enumerate() {
                let hit_state = self.state.clone();
                let command = entry.command;
                let hover_state = self.state.clone();
                layers.push(AnyView::new(
                    AnyView::new(
                        cove_ui::compose::primitives::controls::Button::new(
                            cove_ui::composition::EmptyView {},
                            move || hit_state.execute(command),
                        )
                        .button_style(plain()),
                    )
                    .on_hover(move |entering| {
                        hover_state.active.set(if entering { index as i32 } else { -1 });
                    }),
                ));
            }
            for (index, entry) in ENTRIES.iter().enumerate() {
                let label_state = self.state.clone();
                let command = entry.command;
                let hover_state = self.state.clone();
                layers.push(AnyView::new(
                    AnyView::new(
                        cove_ui::compose::primitives::controls::Button::title(
                            entry.label,
                            move || label_state.execute(command),
                        )
                        .button_style(plain())
                        .foreground_color(Color::WHITE),
                    )
                    .on_hover(move |entering| {
                        hover_state.active.set(if entering { index as i32 } else { -1 });
                    }),
                ));
            }
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
        let drop = FileDropZone::new(extract_dropped, stack)
            .accepted_extensions(ARCHIVE_EXTENSIONS)
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
                2 if self.expanded => (self.size / 2.0 - 32.0, self.size / 2.0 - 32.0, 64.0, 64.0),
                i if self.expanded && (3..9).contains(&i) => {
                    let p = item_center(i - 3, self.size);
                    (p.x - 31.0, p.y - 31.0, 62.0, 62.0)
                }
                i if self.expanded && (9..15).contains(&i) => {
                    let p = item_center(i - 9, self.size);
                    (p.x - 40.0, p.y + ITEM_RADIUS + 3.0, 80.0, 20.0)
                }
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
        let r = DOT / 2.0 - 3.0;
        let lit = self.targeted || self.dot_hovered;

        // Idle dots sit at roughly half opacity so they never compete with the
        // content underneath; hovering or dragging brings them to full.
        let alpha = if self.targeted { 0.95 } else if lit { 0.88 } else { 0.48 };
        let body = if self.targeted { rgb(0x2E, 0xC8, 0x86) } else { rgb(0x1B, 0x25, 0x22) };

        let _ = list.add_disk_shaded(center, r, move |_x, _y| {
            Color { a: alpha, ..body }
        });
        // Fine light edge keeps the dot legible on both light and dark walls.
        let _ = list.add_shaded_annulus(center, r - 1.2, r, |_x, _y| {
            Color { a: if lit { 0.55 } else { 0.30 }, ..Color::WHITE }
        });

        self.draw_icon(list, ENTRIES.len(), "archive", cx, cy, 22.0, 0.98);
    }

    fn paint_disc(&self, list: &mut DrawList, origin: Point) {
        let cx = origin.x + DISC / 2.0;
        let cy = origin.y + DISC / 2.0;
        let center = Point::new(cx, cy);
        let r = DISC / 2.0 - 2.0;

        // Frosted dark disc: a subtle vertical gradient reads as a translucent
        // material rather than a flat grey circle.
        let _ = list.add_disk_shaded(center, r, move |_x, y| {
            let t = ((y - cy) / r * 0.5 + 0.5).clamp(0.0, 1.0);
            Color {
                r: 0.055 + 0.025 * (1.0 - t),
                g: 0.075 + 0.030 * (1.0 - t),
                b: 0.070 + 0.028 * (1.0 - t),
                a: 0.94,
            }
        });
        let _ = list.add_shaded_annulus(center, r - 1.5, r, |_x, _y| {
            Color { a: 0.20, ..Color::WHITE }
        });

        // Centre: a quiet recessed well holding the close glyph.
        let well = 31.0;
        let _ = list.add_disk_shaded(center, well, |_x, _y| {
            Color { r: 1.0, g: 1.0, b: 1.0, a: 0.07 }
        });
        self.draw_icon(list, ENTRIES.len() + 1, "x", cx, cy, 20.0, 0.72);

        // Ring actions.
        for (index, entry) in ENTRIES.iter().enumerate() {
            let p = item_center(index, DISC);
            let pcx = origin.x + p.x;
            let pcy = origin.y + p.y;
            let pc = Point::new(pcx, pcy);
            let selected = self.active == index as i32;

            // Selected actions get an emerald fill; idle ones a faint well.
            let fill = if selected { rgb(0x2E, 0xB8, 0x7E) } else { Color::WHITE };
            let fill_alpha = if selected { 0.92 } else { 0.10 };
            let _ = list.add_disk_shaded(pc, ITEM_RADIUS, move |_x, _y| {
                Color { a: fill_alpha, ..fill }
            });
            let _ = list.add_shaded_annulus(pc, ITEM_RADIUS - 1.0, ITEM_RADIUS, |_x, _y| {
                Color { a: if selected { 0.55 } else { 0.14 }, ..Color::WHITE }
            });
            self.draw_icon(list, index, entry.icon, pcx, pcy, 22.0, 0.96);
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
