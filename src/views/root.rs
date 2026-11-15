//! One window, in three places at once: a location index down the left edge,
//! the packages living in that location, and — once one is opened — the
//! archive's own contents. The sidebar answers "where am I", never "what
//! happened"; progress belongs to the package it is unpacking.

use std::cmp::Ordering;
use std::rc::Rc;

use cove_ui::compose::adaptive::SurfaceEnvironment;
use cove_ui::compose::modifiers::layout::EdgeInsets;
use cove_ui::compose::modifiers::style::AppearanceExt;
use cove_ui::compose::prelude::*;
use cove_ui::compose::primitives::controls::Toggle;
use cove_ui::compose::primitives::data::table::{ColumnVisibility, ComparisonResult};
use cove_ui::compose::primitives::feedback::ProgressView;
use cove_ui::compose::primitives::layout::{
    ScrollView, SplitAxis, SplitConstraints, SplitView,
};
use cove_ui::compose::primitives::media::ui_icon_catalog::{action, file, media, navigation};
use cove_ui::compose::primitives::overlays::portal::LayerRoot;
use cove_ui::compose::primitives::overlays::{Modal, ModalOptions};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, GhostButtonStyle, PlainButtonStyle,
};
use cove_ui::compose::theme::ThemeViewExt;
use cove_ui::composition::{
    BoxView, DynamicField, EmptyView, HorizontalAlignment, Scope, Spacer, TupleView,
    VerticalAlignment, views,
};
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};
use cove_ui::core::Color;

use crate::browser::FileItem;
use crate::model::{
    AppModel, LoadedArchive, PackageItem, Phase, Tone, ARCHIVE_EXTENSIONS,
};
use crate::theme;
use mugenzip_core::format_size;

const TOOLBAR: f32 = 56.0;
const CRUMB_BAR: f32 = 34.0;
const PROGRESS_BAR: f32 = 34.0;
const GUTTER: f32 = 20.0;
const ROW_HEIGHT: f32 = 38.0;
const HEADER_HEIGHT: f32 = 30.0;
/// The drop invitation is a fixed plate in the middle of the card, the way the
/// empty state reads in the prototype.
const DROP_WIDTH: f32 = 420.0;
const DROP_HEIGHT: f32 = 196.0;

pub struct RootView {
    pub model: Rc<AppModel>,
}

impl View for RootView {
    fn body(&self, cx: &Scope) -> AnyView {
        self.model.ensure_platform_hooks();
        self.model.bootstrap_location();
        let model = self.model.clone();

        // The window's material runs under the traffic lights, so every band
        // starts below the safe area — the way a Mac window behaves.
        let safe_top = cx
            .composition_value(&SurfaceEnvironment::key())
            .safe_area
            .top
            .max(0.0);

        let drop = model.clone();
        let targeted = model.clone();
        let shell = model.clone();
        let workspace_view = AnyView::new(
            FileDropZone::new(
                move |files| drop.open_dropped(files),
                workspace(shell, safe_top),
            )
            .accepted_extensions(ARCHIVE_EXTENSIONS)
            .maximum_files(1)
            .shows_chrome(false)
            .is_targeted(model.drop_targeted.binding())
            .on_targeted(move |is_targeted| targeted.drop_targeted.set(is_targeted))
            .fill()
            .theme(theme::paper()),
        );

        // The settings sheet is a portal: it covers the window without taking
        // part in the workspace's layout.
        let mut options = ModalOptions::new();
        options.geometry.width = 468.0;
        options.geometry.height = 560.0;
        let settings = AnyView::new(
            Modal::new(
                model.settings_open.binding(),
                settings_panel(model.clone()),
            )
            .options(options)
            .debug_name("settings-sheet"),
        );

        // `LayerRoot` mounts the portal host, which is what makes sheets and
        // menus draw above the window instead of inside the layout.
        AnyView::new(
            AnyView::new(LayerRoot::new(AnyView::new(
                BoxView::new(
                    FlexDirection::Column,
                    Align::Stretch,
                    Justify::FlexStart,
                    Wrap::NoWrap,
                    0.0,
                    AnyView::new(TupleView::new(vec![workspace_view, settings])),
                )
                .fill(),
            )))
            .theme(theme::paper()),
        )
    }

    fn dynamic_properties(&self) -> Vec<DynamicField> {
        self.model.dynamic_fields()
    }
}

// ---------------------------------------------------------------------------
// shell
// ---------------------------------------------------------------------------

/// The window never changes its frame: the index stays on the left, the card
/// keeps its 8 pt air gap, and only the card's内容 changes. The split's divider
/// lane is invisible — the card's edge separates — but it still drags.
fn workspace(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    // Padding has to live on a container node: put it on the card itself and
    // the fill swallows its own margin, leaving the card flush to the edge.
    let card = AnyView::new(
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                0.0,
                AnyView::new(views![AnyView::new(content(model.clone(), safe_top))
                    .semantic_background(SemanticColorRef::surface())
                    .corner_radius(14.0)
                    .semantic_border(SemanticColorRef::border(), 0.5)
                    .clipped()
                    .fill_width()
                    .fill_height()]),
            )
            .fill(),
        )
        .padding_insets(EdgeInsets::all(8.0)),
    );

    AnyView::new(
        SplitView::resizable(
            SplitAxis::Horizontal,
            model.sidebar_fraction.binding(),
            sidebar(model.clone(), safe_top),
            card,
        )
        .divider_thickness(0.0)
        // The handle always paints its own 1pt hairline, whatever the
        // thickness says — clear the color so the card edge is the only
        // boundary you see, while the 7pt lane still drags.
        .divider_color(Color::CLEAR)
        .constraints(SplitConstraints {
            minimum_first: 180.0,
            minimum_second: 460.0,
            ..SplitConstraints::new(0.14, 0.32)
        })
        .fill(),
    )
}

/// The sidebar is a place index: the folders the window can look into. It
/// never lists an archive's contents — that is the main area's job — and it
/// never narrates the job, because the job reports where the job happens.
fn sidebar(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    let current = model.location.get();
    let mut rows: Vec<AnyView> = Vec::new();
    rows.push(section_label("位置", 0.0));

    for (label, path) in model.locations() {
        let count = model.package_count(&path);
        let selected = current.as_ref() == Some(&path);
        let pick = model.clone();
        let target = path.clone();
        let trailing = if count == 0 {
            "—".to_owned()
        } else {
            count.to_string()
        };
        rows.push(side_row(
            file::folder_open(),
            label,
            Some(trailing),
            selected,
            move || pick.set_location(target.clone()),
        ));
    }

    let add = model.clone();
    rows.push(sub_row_icon(
        file::folder_open(),
        "添加文件夹…",
        move || add.choose_location(),
    ));

    // The index can outgrow the pane once the user adds folders, so it scrolls
    // inside it; the settings entry stays pinned to the bottom edge.
    let nav = AnyView::new(
        ScrollView::vertical(
            AnyView::new(vstack(0.0, rows))
                .padding_insets(EdgeInsets::new(safe_top + 16.0, 8.0, 12.0, 8.0))
                .fill_width(),
        )
        .combine_accessibility(false),
    )
    .fill_width()
    .flex_with(1.0, 1.0, Some(0.0));

    // The pinned row must keep its own height; without an explicit shrink the
    // scroll pane below grabs the whole column and swallows its clicks.
    let settings = AnyView::new(settings_entry(model.clone())).shrink(0.0);

    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views![AnyView::new(nav), AnyView::new(settings)]),
        )
        .fill_width()
        .fill_height(),
    )
}

/// The pinned bottom entry: a quiet row, highlighted only while its sheet is
/// up — the whole point is that it stays out of the way the rest of the time.
fn settings_entry(model: Rc<AppModel>) -> AnyView {
    let open = model.clone();
    let highlighted = model.settings_open.get();
    let tint = if highlighted {
        SemanticColorRef::on_surface()
    } else {
        SemanticColorRef::on_surface_variant()
    };

    let button = AnyView::new(
        Button::new(
            Row::new(
                VerticalAlignment::Center,
                9.0,
                AnyView::new(views!(
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(action::settings()),
                        Some(15.0),
                        None,
                    ))
                    .semantic_foreground(tint.clone()),
                    Text::new("设置")
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(tint),
                    Spacer::new(0.0),
                )),
            )
            .fill_width(),
            move || open.open_settings(),
        )
        .accessibility_text("设置")
        .tooltip("设置")
        .button_style(GhostButtonStyle)
        .control_size(ControlSize::Small)
        .fill_width(),
    )
    .frame(None, None, None, Some(32.0), None, None);

    AnyView::new(
        AnyView::new(button)
            .padding_insets(EdgeInsets::new(6.0, 8.0, 12.0, 8.0))
            .fill_width(),
    )
}

// ---------------------------------------------------------------------------
// content
// ---------------------------------------------------------------------------

fn content(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    let inner = if model.archive.get().is_some() {
        package_view(model.clone(), safe_top)
    } else {
        browse_view(model.clone(), safe_top)
    };

    AnyView::new(
        Column::new(HorizontalAlignment::Leading, 0.0, AnyView::new(inner))
            .flex(1.0)
            .fill_height()
            .semantic_background(SemanticColorRef::surface_sunken()),
    )
}

// ---------------------------------------------------------------------------
// browse: the packages inside a location
// ---------------------------------------------------------------------------

fn browse_view(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    let packages = model.visible_packages();
    let empty = model.packages.get().is_empty();
    let searching = !model.search.get().string_value().trim().is_empty();

    let middle: AnyView = if empty {
        // Nothing here yet: the whole card becomes one invitation, which is
        // also the drag target the window already listens for.
        drop_invitation(model.clone())
    } else if packages.is_empty() {
        centered_note("没有匹配的压缩包")
    } else {
        package_area(model.clone())
    };

    let selection = model.package_selection.get();
    let batch_bar: AnyView = if selection.is_empty() {
        AnyView::new(EmptyView)
    } else {
        batch_bar(model.clone())
    };

    AnyView::new(
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                0.0,
                AnyView::new(TupleView::new(vec![
                    toolbar_browse(model.clone(), safe_top, searching),
                    middle,
                    batch_bar,
                ])),
            )
            .flex(1.0)
            .fill_width()
            .fill_height(),
        )
        .debug_name("browse"),
    )
}

/// Table + padding. Kept apart so the empty branches stay readable.
fn package_area(model: Rc<AppModel>) -> AnyView {
    AnyView::new(
        Column::new(HorizontalAlignment::Leading, 0.0, AnyView::new(package_table(model)))
            .padding_insets(EdgeInsets::new(4.0, GUTTER, 12.0, GUTTER))
            .flex(1.0)
            .fill_width(),
    )
}

fn drop_invitation(model: Rc<AppModel>) -> AnyView {
    let open = model.clone();
    let content = AnyView::new(
        BoxView::new(
            FlexDirection::Column,
            Align::Center,
            Justify::Center,
            Wrap::NoWrap,
            0.0,
            AnyView::new(
                Column::new(
                    HorizontalAlignment::Center,
                    9.0,
                    AnyView::new(views!(
                        AnyView::new(ButtonIcon::new(
                            ButtonIconSource::resource(file::archive()),
                            Some(26.0),
                            None,
                        ))
                        .semantic_foreground(SemanticColorRef::on_surface_variant()),
                        Text::new("打开压缩包")
                            .semantic_font(SemanticFontRef::body_strong())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        Text::new("拖到这里，或按 ⌘O")
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    )),
                )
                .frame(None, None, Some(DROP_WIDTH), None, None, None),
            ),
        )
        .fill(),
    );

    // A plate, not a page: the invitation is one quiet target in the middle of
    // an otherwise empty card, exactly where the eye already is.
    let plate = AnyView::new(
        AnyView::new(
            Button::new(content, move || open.open_dialog())
                .accessibility_text("打开压缩包")
                .button_style(PlainButtonStyle)
                .frame(None, None, Some(DROP_WIDTH), Some(DROP_HEIGHT), None, None),
        )
        .frame(None, None, Some(DROP_WIDTH), Some(DROP_HEIGHT), None, None)
        .corner_radius(12.0)
        .semantic_border(SemanticColorRef::border(), 1.0)
        .semantic_background(SemanticColorRef::surface())
        .clipped(),
    );

    AnyView::new(
        BoxView::new(
            FlexDirection::Column,
            Align::Center,
            Justify::Center,
            Wrap::NoWrap,
            0.0,
            AnyView::new(views![AnyView::new(plate)]),
        )
        .flex(1.0)
        .fill_width()
        .fill_height(),
    )
}

fn centered_note(text: &str) -> AnyView {
    AnyView::new(
        Column::new(
            HorizontalAlignment::Center,
            0.0,
            AnyView::new(views![
                Spacer::new(0.0),
                Text::new(text.to_owned())
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
                Spacer::new(0.0),
            ]),
        )
        .flex(1.0)
        .fill_width(),
    )
}

/// "已选 2 个压缩包 … 解压到 HaHa ▾ … 解压" — appears only while a tick exists,
/// so the resting state of the list is just the list.
fn batch_bar(model: Rc<AppModel>) -> AnyView {
    let (count, bytes) = model.package_scope();
    let run = model.clone();
    let clear = model.clone();

    let summary = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            8.0,
            AnyView::new(views!(
                Text::new(format!("已选 {count} 个压缩包"))
                    .semantic_font(SemanticFontRef::label())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Text::new(format_size(bytes))
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )),
        )
        .frame_min_width(0.0),
    );

    let tools = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            14.0,
            AnyView::new(views![
                dest_chip(model.clone()),
                quiet_button("取消选择", move || clear.clear_packages()),
                extract_button(!model.is_busy(), move || run.batch_extract()),
            ]),
        )
        .frame_min_width(0.0),
    );

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            8.0,
            AnyView::new(views![summary, Spacer::new(0.0), tools]),
        )
        .padding_insets(EdgeInsets::new(0.0, 12.0, 0.0, 10.0))
        .frame_height(46.0)
        .fill_width()
        .corner_radius(12.0)
        .semantic_background(SemanticColorRef::surface())
        .semantic_border(SemanticColorRef::border(), 0.5)
        .padding_insets(EdgeInsets::new(0.0, GUTTER, 10.0, GUTTER)),
    )
}

fn package_table(model: Rc<AppModel>) -> AnyView {
    let mut options = TableOptions::default();
    options.selection = Some(Selection::multiple(model.package_selection.binding()));
    options.sort = Some(model.table_sort.binding());
    options.frozen_column_count = 1;
    options.layout = TableLayout {
        row_height: ROW_HEIGHT,
        header_height: HEADER_HEIGHT,
        is_striped: false,
        separator_inset: Some(0.0),
        fills_width: true,
        is_card: false,
        empty_title: "没有匹配的压缩包".into(),
    };

    let open = model.clone();
    let table = Table::new(
        model.visible_packages(),
        |row: &PackageItem| AppModel::package_key(&row.path),
        package_columns(model.clone()),
    )
    .expect("the package table columns are static with unique ids")
    .options(options)
    .on_activate(move |row: &PackageItem| open.open(row.path.clone()))
    .debug_name("package-table");

    AnyView::new(
        AnyView::new(table.flex(1.0).fill_height())
            .corner_radius(10.0)
            .semantic_border(SemanticColorRef::border(), 1.0)
            .semantic_background(SemanticColorRef::surface())
            .clipped()
            .flex(1.0),
    )
}

fn package_columns(model: Rc<AppModel>) -> Vec<TableColumn<PackageItem>> {
    let status_for = move |row: &PackageItem| -> AnyView {
        if row.extracted_to.is_some() {
            AnyView::new(
                Text::new("已解压")
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::accent()),
            )
        } else {
            AnyView::new(
                Text::new("未处理")
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )
        }
    };

    let extract = model.clone();
    vec![
        TableColumn::new("name", "名称", move |row: &PackageItem| {
            let menu = PackageMenu::entries(extract.clone(), row.path.clone());
            AnyView::new(
                Row::new(
                    VerticalAlignment::Center,
                    9.0,
                    AnyView::new(views!(
                        AnyView::new(ButtonIcon::new(
                            ButtonIconSource::resource(file::archive()),
                            Some(15.0),
                            None,
                        ))
                        .semantic_foreground(SemanticColorRef::on_surface_variant()),
                        Text::new(row.name.clone())
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::body())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                    )),
                )
                .frame_min_width(0.0)
                .context_menu(menu),
            )
        })
        .layout(TableColumnLayout {
            width: 340.0,
            min_width: 180.0,
            max_width: 2400.0,
            is_flex: true,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_text(&left.name, &right.name)),
        TableColumn::new("size", "大小", |row: &PackageItem| {
            AnyView::new(
                Text::new(row.size.clone())
                    .semantic_font(SemanticFontRef::mono())
                    .semantic_foreground(SemanticColorRef::on_surface_variant()),
            )
        })
        .layout(TableColumnLayout {
            width: 92.0,
            alignment: TextAlignment::Trailing,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_number(left.bytes, right.bytes)),
        TableColumn::new("status", "状态", status_for)
            .layout(TableColumnLayout {
                width: 78.0,
                ..TableColumnLayout::default()
            })
            .visibility(ColumnVisibility::Required)
            .compare(|left, right| {
                compare_number(left.extracted_to.is_some() as u64, right.extracted_to.is_some() as u64)
            }),
        TableColumn::new("modified", "修改时间", |row: &PackageItem| {
            AnyView::new(
                Text::new(row.modified.clone())
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )
        })
        .layout(TableColumnLayout {
            width: 124.0,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_text(&left.modified, &right.modified)),
    ]
}

/// The per-row commands, in the same language as the archive's own rows.
struct PackageMenu;

impl PackageMenu {
    fn entries(model: Rc<AppModel>, path: std::path::PathBuf) -> Vec<MenuEntry> {
        let open = model.clone();
        let open_path = path.clone();
        vec![
            MenuEntry::Item(MenuItem::new(1, "打开", move || open.open(open_path.clone()))),
            MenuEntry::separator(2),
            MenuEntry::Item(MenuItem::new(
                3,
                "在文件管理器中显示",
                move || crate::model::reveal_parent(&path),
            )),
        ]
    }
}

// ---------------------------------------------------------------------------
// a package: its own toolbar, crumbs and rows
// ---------------------------------------------------------------------------

fn package_view(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    let busy = model.phase.get() == Phase::Extracting;
    let progress: AnyView = if busy {
        progress_strip(model.clone())
    } else {
        AnyView::new(EmptyView)
    };

    AnyView::new(
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                0.0,
                AnyView::new(views![
                    toolbar_package(model.clone(), safe_top),
                    progress,
                    crumb_bar(model.clone()),
                    list_area(model.clone()),
                ]),
            )
            .flex(1.0)
            .fill_width(),
        )
        .debug_name("package"),
    )
}

fn toolbar_package(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    let Some(archive) = model.archive.get() else {
        return AnyView::new(EmptyView);
    };
    let close = model.clone();
    let back_button = AnyView::new(
        Button::new(
            AnyView::new(ButtonIcon::new(
                ButtonIconSource::resource(navigation::chevron_left()),
                Some(15.0),
                None,
            ))
            .semantic_foreground(SemanticColorRef::on_surface()),
            move || close.close_archive(),
        )
        .enabled(!model.is_busy())
        .accessibility_text("返回位置")
        .tooltip("返回位置")
        .button_style(PlainButtonStyle)
        .control_size(ControlSize::Small),
    );

    let title_block = AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            2.0,
            AnyView::new(views!(
                Text::new(archive.name.clone())
                    .line_limit(Some(1))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::title())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Text::new(identity_subtitle(&model, &archive))
                    .line_limit(Some(1))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )),
        )
        .frame_min_width(0.0),
    );

    let open = model.clone();
    let open_action = toolbar_action(file::archive(), "打开", true, false, move || open.open_dialog());

    let run = model.clone();
    let extract = extract_button(!model.is_busy(), move || run.extract());

    let tools = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            14.0,
            AnyView::new(views![
                search_field(model.clone()),
                open_action,
                dest_chip(model.clone()),
                extract,
                more_pill(model.clone()),
            ]),
        )
        .frame_min_width(0.0),
    );

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views![back_button, title_block, Spacer::new(0.0), tools]),
        )
        .padding_insets(EdgeInsets::new(safe_top + 6.0, GUTTER, 6.0, GUTTER))
        .frame(None, None, None, Some(TOOLBAR + safe_top), None, None)
        .fill_width(),
    )
}

fn toolbar_browse(model: Rc<AppModel>, safe_top: f32, searching: bool) -> AnyView {
    let title_block = AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            2.0,
            AnyView::new(views!(
                Text::new(model.location_name())
                    .line_limit(Some(1))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::title())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Text::new(if searching {
                    format!("{} · 筛选后", model.location_summary())
                } else {
                    model.location_summary()
                })
                .line_limit(Some(1))
                .truncating_tail()
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )),
        )
        .frame_min_width(0.0),
    );

    let open = model.clone();
    let open_action = toolbar_action(file::archive(), "打开", true, false, move || open.open_dialog());

    let tools = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            14.0,
            AnyView::new(views![
                search_field(model.clone()),
                open_action,
                more_pill(model.clone()),
            ]),
        )
        .frame_min_width(0.0),
    );

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views![title_block, Spacer::new(0.0), tools]),
        )
        .padding_insets(EdgeInsets::new(safe_top + 6.0, GUTTER, 6.0, GUTTER))
        .frame(None, None, None, Some(TOOLBAR + safe_top), None, None)
        .fill_width(),
    )
}

/// Metadata while the archive is open and quiet; the running task while it is
/// not; the failure instead of the metadata when something went wrong.
fn identity_subtitle(model: &AppModel, archive: &LoadedArchive) -> String {
    let status = model.status.get();
    if status.tone == Tone::Error {
        return if status.detail.trim().is_empty() {
            status.text
        } else {
            format!("{} · {}", status.text, status.detail)
        };
    }
    archive.meta_line()
}

/// "解压到 HaHa ▾" — one line of text, a menu behind it. The destination is
/// stated next to the verb it changes, always, and never as a list somewhere
/// else in the window.
fn dest_chip(model: Rc<AppModel>) -> AnyView {
    let name = model.destination_name();
    let menu = destination_menu(model);
    AnyView::new(
        MenuButton::new(
            menu,
            AnyView::new(
                Row::new(
                    VerticalAlignment::Center,
                    5.0,
                    AnyView::new(views!(
                        Text::new("解压到")
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        Text::new(name)
                            .line_limit(Some(1))
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::label())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        AnyView::new(ButtonIcon::new(
                            ButtonIconSource::resource(navigation::chevron_down()),
                            Some(11.0),
                            None,
                        ))
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    )),
                ),
            ),
        )
        .ghost()
        .accessibility_label("更改解压位置")
        .tooltip("更改解压位置")
        .frame_height(26.0),
    )
}

fn destination_menu(model: Rc<AppModel>) -> Vec<MenuEntry> {
    let current = model.destination.get();
    let mut entries: Vec<MenuEntry> = Vec::new();
    for (index, (label, path)) in model.destination_choices().into_iter().enumerate() {
        let pick = model.clone();
        let target = path.clone();
        let selected = current.as_ref() == Some(&path);
        entries.push(MenuEntry::Item(
            MenuItem::new(index as u64 + 10, label, move || {
                pick.set_destination(target.clone())
            })
            .selected(selected),
        ));
    }
    let choose = model.clone();
    entries.push(MenuEntry::separator(90));
    entries.push(MenuEntry::Item(MenuItem::new(
        91,
        "选择其他文件夹…",
        move || choose.choose_destination(),
    )));
    entries
}

fn search_field(model: Rc<AppModel>) -> AnyView {
    AnyView::new(
        SearchField::new(model.search.binding())
            .placeholder("搜索")
            .frame_width(128.0)
            .control_size(ControlSize::Small),
    )
}

/// The one accent-tinted action in the window. Everything else waits its turn
/// in the neutral ink.
fn extract_button(enabled: bool, action: impl Fn() + 'static) -> AnyView {
    AnyView::new(
        Button::new(
            Row::new(
                VerticalAlignment::Center,
                5.0,
                AnyView::new(views!(
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(action::download()),
                        Some(13.0),
                        None,
                    ))
                    .semantic_foreground(SemanticColorRef::accent()),
                    Text::new("解压")
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(SemanticColorRef::accent()),
                )),
            ),
            action,
        )
        .enabled(enabled)
        .accessibility_text("解压")
        .tooltip("解压到所选位置")
        .button_style(GhostButtonStyle)
        .control_size(ControlSize::Small),
    )
}

fn quiet_button(title: &'static str, action: impl Fn() + 'static) -> AnyView {
    AnyView::new(
        Button::new(
            Text::new(title)
                .semantic_font(SemanticFontRef::label())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
            action,
        )
        .accessibility_text(title)
        .button_style(GhostButtonStyle)
        .control_size(ControlSize::Small),
    )
}

/// Toolbar language: glyph over a small label, no chrome until the pointer
/// arrives.
fn toolbar_action(
    glyph: cove_ui::imaging::BundleImageResource,
    title: &'static str,
    enabled: bool,
    accent: bool,
    action: impl Fn() + 'static,
) -> AnyView {
    let tint = if accent {
        SemanticColorRef::accent()
    } else {
        SemanticColorRef::on_surface_variant()
    };
    AnyView::new(
        Button::new(
            Row::new(
                VerticalAlignment::Center,
                6.0,
                AnyView::new(views!(
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(glyph),
                        Some(15.0),
                        None,
                    ))
                    .semantic_foreground(tint.clone()),
                    Text::new(title)
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(tint),
                )),
            ),
            action,
        )
        .enabled(enabled)
        .accessibility_text(title)
        .tooltip(title)
        .button_style(GhostButtonStyle)
        .control_size(ControlSize::Small),
    )
}

/// The secondary control: a hairline pill that reads "更多 ⌄".
fn more_pill(model: Rc<AppModel>) -> AnyView {
    AnyView::new(
        MenuButton::new(
            menu_entries(model),
            AnyView::new(
                Row::new(
                    VerticalAlignment::Center,
                    6.0,
                    AnyView::new(views!(
                        AnyView::new(ButtonIcon::new(
                            ButtonIconSource::resource(navigation::menu()),
                            Some(15.0),
                            None,
                        ))
                        .semantic_foreground(SemanticColorRef::on_surface_variant()),
                        Text::new("更多")
                            .semantic_font(SemanticFontRef::label())
                            .semantic_foreground(SemanticColorRef::on_surface_variant()),
                    )),
                ),
            ),
        )
        .ghost()
        .accessibility_label("更多操作")
        .tooltip("更多操作")
        .frame_height(28.0),
    )
}

/// The overflow command list. Commands that need a subject are absent until
/// there is one — never present as a dead title.
fn menu_entries(model: Rc<AppModel>) -> Vec<MenuEntry> {
    let open = model.clone();
    let loaded = model.archive.get().is_some();
    let mut entries = vec![MenuEntry::Item(
        MenuItem::new(4, "打开压缩包…", move || open.open_dialog()).shortcut("⌘O"),
    )];
    if !loaded {
        let refresh = model.clone();
        entries.push(MenuEntry::separator(8));
        entries.push(MenuEntry::Item(MenuItem::new(9, "刷新列表", move || {
            refresh.refresh_packages()
        })));
        return entries;
    }

    let close = model.clone();
    let wrap_on = model.wrap_in_folder.get();
    let toggle = model.clone();
    let choose = model.clone();
    let all = model.clone();
    let copy = model.clone();
    let reveal = model.clone();
    entries.push(MenuEntry::separator(5));
    entries.push(MenuEntry::Item(MenuItem::new(10, "返回位置列表", move || {
        close.close_archive()
    })));
    entries.push(MenuEntry::Item(MenuItem::new(1, "更改解压位置…", move || {
        choose.choose_destination()
    })));
    entries.push(MenuEntry::Item(
        MenuItem::new(2, "解压到同名文件夹", move || toggle.toggle_wrap()).selected(wrap_on),
    ));
    entries.push(MenuEntry::separator(3));
    entries.push(MenuEntry::Item(
        MenuItem::new(6, "全选", move || all.select_all()).shortcut("⌘A"),
    ));
    entries.push(MenuEntry::Item(MenuItem::new(7, "复制档案路径", move || {
        copy.copy_path(String::new())
    })));
    entries.push(MenuEntry::Item(MenuItem::new(11, "在文件管理器中显示", move || {
        reveal.reveal_last()
    })));
    entries
}

// ---------------------------------------------------------------------------
// progress / breadcrumb / list
// ---------------------------------------------------------------------------

fn progress_strip(model: Rc<AppModel>) -> AnyView {
    let progress = model.progress.get();
    let cancel = model.clone();
    let current = if progress.current_file.trim().is_empty() {
        progress.label.clone()
    } else {
        progress.current_file.clone()
    };
    let percent = (progress.fraction * 100.0).round();

    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            4.0,
            AnyView::new(views!(
                Row::new(
                    VerticalAlignment::Center,
                    10.0,
                    AnyView::new(views!(
                        Text::new("解压中")
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        Text::new(progress.label.clone())
                            .semantic_font(SemanticFontRef::mono())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        Text::new(current)
                            .line_limit(Some(1))
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted())
                            .frame_min_width(0.0)
                            .flex(1.0),
                        Text::new(format!("{percent:.0}%"))
                            .semantic_font(SemanticFontRef::mono())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        Button::new(
                            Text::new("取消")
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            move || cancel.cancel_extract(),
                        )
                        .accessibility_text("取消解压")
                        .button_style(GhostButtonStyle)
                        .control_size(ControlSize::Small),
                    )),
                ),
                ProgressView::new(Some(progress.fraction))
                    .height(3.0)
                    .tone(StatusTone::Info)
                    .fill_width(),
            )),
        )
        .padding_insets(EdgeInsets::new(8.0, GUTTER, 8.0, GUTTER))
        .frame(None, None, None, Some(PROGRESS_BAR), None, None)
        .fill_width(),
    )
}

fn crumb_bar(model: Rc<AppModel>) -> AnyView {
    let directory = model.directory.get();
    let crumbs = model
        .archive
        .get()
        .map(|archive| archive.browser.breadcrumbs(&directory))
        .unwrap_or_default()
        .into_iter()
        .map(|crumb| BreadcrumbItem::new(crumb.path, crumb.name))
        .collect();
    let select = model.clone();
    let breadcrumb: AnyView =
        Breadcrumb::try_new(crumbs, 5, move |path| select.enter_directory(path))
            .map(AnyView::new)
            .unwrap_or_else(|_| AnyView::new(EmptyView));

    // The right half answers "what is in here". Once a run has landed, that
    // answer is replaced by what happened — completion belongs to the package
    // it completed, in the band above its contents.
    let right: AnyView = if let Some(record) = done_record(&model) {
        let reveal = model.clone();
        AnyView::new(
            Row::new(
                VerticalAlignment::Center,
                10.0,
                AnyView::new(views!(
                    Text::new(format!("已完成 · {}", record.at))
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::accent()),
                    Button::new(
                        Text::new("在访达显示")
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::accent()),
                        move || reveal.reveal_last(),
                    )
                    .accessibility_text("在访达显示")
                    .button_style(GhostButtonStyle)
                    .control_size(ControlSize::Small),
                )),
            )
            .frame_min_width(0.0),
        )
    } else {
        let items = model.current_items();
        let folders = items.iter().filter(|item| item.folder).count();
        let files = items.len() - folders;
        let selected = model.selection.get().len();
        AnyView::new(
            Text::new(if selected > 0 {
                format!("已选 {selected} 项")
            } else {
                format!("{folders} 个文件夹 · {files} 个文件")
            })
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
        )
    };

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            8.0,
            AnyView::new(views![
                AnyView::new(breadcrumb).frame_min_width(0.0).flex(1.0),
                right,
            ]),
        )
        .padding_insets(EdgeInsets::new(4.0, GUTTER, 4.0, GUTTER))
        .frame(None, None, None, Some(CRUMB_BAR), None, None)
        .fill_width(),
    )
}

/// The record to advertise as "该包已解压" — only while nothing is running and
/// the last run actually succeeded.
fn done_record(model: &AppModel) -> Option<crate::model::ExtractRecord> {
    if model.phase.get() == Phase::Extracting {
        return None;
    }
    let archive = model.archive.get()?;
    let record = model.last_extraction()?;
    if record.name != archive.name {
        return None;
    }
    if !model.status.get().text.starts_with("解压完成") {
        return None;
    }
    Some(record)
}

fn list_area(model: Rc<AppModel>) -> AnyView {
    AnyView::new(
        Column::new(HorizontalAlignment::Leading, 0.0, AnyView::new(file_table(model)))
            .padding_insets(EdgeInsets::new(4.0, GUTTER, GUTTER, GUTTER))
            .flex(1.0)
            .fill_width(),
    )
}

fn file_table(model: Rc<AppModel>) -> AnyView {
    let searching = !model.search.get().string_value().trim().is_empty();
    let mut options = TableOptions::default();
    options.selection = Some(Selection::multiple(model.selection.binding()));
    options.sort = Some(model.table_sort.binding());
    options.frozen_column_count = 1;
    options.layout = TableLayout {
        row_height: ROW_HEIGHT,
        header_height: HEADER_HEIGHT,
        is_striped: false,
        separator_inset: Some(0.0),
        fills_width: true,
        is_card: false,
        empty_title: if searching {
            "没有匹配的项目".into()
        } else {
            "这个文件夹是空的".into()
        },
    };

    let activate = model.clone();
    let table = Table::new(model.rows(), |row: &FileItem| row.id.clone(), columns(model.clone()))
        .expect("the file table columns are static with unique ids")
        .options(options)
        .on_activate(move |row: &FileItem| activate.activate(row))
        .debug_name("file-table");

    AnyView::new(
        AnyView::new(table.flex(1.0).fill_height())
            .corner_radius(10.0)
            .semantic_border(SemanticColorRef::border(), 1.0)
            .semantic_background(SemanticColorRef::surface())
            .clipped()
            .flex(1.0),
    )
}

fn columns(model: Rc<AppModel>) -> Vec<TableColumn<FileItem>> {
    vec![
        TableColumn::new("name", "名称", move |row: &FileItem| {
            let (glyph, tint) = file_glyph(&row.ext, row.folder);
            let font = if row.folder {
                SemanticFontRef::body_strong()
            } else {
                SemanticFontRef::body()
            };
            let menu = row_menu(model.clone(), row.id.clone());
            AnyView::new(
                Row::new(
                    VerticalAlignment::Center,
                    9.0,
                    AnyView::new(views!(
                        AnyView::new(ButtonIcon::new(
                            ButtonIconSource::resource(glyph),
                            Some(15.0),
                            None,
                        ))
                        .semantic_foreground(tint),
                        Text::new(row.name.clone())
                            .truncating_tail()
                            .semantic_font(font)
                            .semantic_foreground(SemanticColorRef::on_surface()),
                    )),
                )
                .frame_min_width(0.0)
                .context_menu(menu),
            )
        })
        .layout(TableColumnLayout {
            width: 320.0,
            min_width: 180.0,
            max_width: 2000.0,
            is_flex: true,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_text(&left.name, &right.name)),
        TableColumn::new("size", "大小", |row: &FileItem| {
            AnyView::new(
                Text::new(if row.folder {
                    String::new()
                } else {
                    row.size.clone()
                })
                .semantic_font(SemanticFontRef::mono())
                .semantic_foreground(SemanticColorRef::on_surface_variant()),
            )
        })
        .layout(TableColumnLayout {
            width: 92.0,
            alignment: TextAlignment::Trailing,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_number(left.bytes, right.bytes)),
        TableColumn::new("modified", "修改日期", |row: &FileItem| {
            AnyView::new(
                Text::new(if row.folder {
                    String::new()
                } else {
                    row.modified.clone()
                })
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )
        })
        .layout(TableColumnLayout {
            width: 132.0,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_text(&left.modified, &right.modified)),
    ]
}

fn row_menu(model: Rc<AppModel>, id: String) -> Vec<MenuEntry> {
    let extract = model.clone();
    let extract_id = id.clone();
    let copy = model.clone();
    vec![
        MenuEntry::Item(MenuItem::new(1, "解压此项", move || {
            extract.extract_item(extract_id.clone())
        })),
        MenuEntry::separator(2),
        MenuEntry::Item(MenuItem::new(3, "复制路径", move || copy.copy_path(id.clone()))),
    ]
}

// ---------------------------------------------------------------------------
// settings sheet
// ---------------------------------------------------------------------------

/// A panel, not a second window: a title, grouped rows, and room for the
/// settings that do not exist yet. Every control here is real; the groups that
/// are still empty say so instead of offering switches that do nothing.
fn settings_panel(model: Rc<AppModel>) -> AnyView {
    let close = model.clone();
    let header = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            8.0,
            AnyView::new(views![
                Text::new("设置")
                    .semantic_font(SemanticFontRef::title())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Spacer::new(0.0),
                Button::new(
                    Text::new("完成")
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(SemanticColorRef::accent()),
                    move || close.close_settings(),
                )
                .accessibility_text("关闭设置")
                .button_style(PlainButtonStyle)
                .control_size(ControlSize::Small),
            ]),
        )
        .fill_width(),
    );

    let extract_section = settings_section(
        "解压",
        vec![
            settings_toggle(
                "解压到同名文件夹",
                "每个压缩包解到以包名命名的文件夹，重名时自动加序号",
                model.wrap_in_folder.binding(),
            ),
            settings_toggle(
                "完成后在文件管理器中显示",
                "解压结束时打开输出目录",
                model.reveal_after_extract.binding(),
            ),
        ],
    );

    let appearance_section = settings_section(
        "外观",
        vec![
            settings_info("主题", "纸白", "深色主题与自定义配色随后加入"),
            settings_info("字体大小", "跟随系统", "字号调节随后加入"),
        ],
    );

    let language_section = settings_section(
        "语言",
        vec![settings_info(
            "界面语言",
            "简体中文",
            "English、日本語 等多语言随后加入",
        )],
    );

    let compress_section = settings_section(
        "压缩",
        vec![settings_info(
            "压缩级别",
            "尚未实现",
            "新建压缩包上线后在此设置级别与默认格式",
        )],
    );

    let legend = AnyView::new(
        Text::new("设置项会随功能逐步开放；当前可用的都已在此生效。")
            .line_limit(Some(2))
            .truncating_tail()
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
    );

    let body = AnyView::new(vstack(
        8.0,
        vec![
            extract_section,
            appearance_section,
            language_section,
            compress_section,
            legend,
        ],
    ))
    .fill_width();

    AnyView::new(
        BoxView::new(
            FlexDirection::Column,
            Align::Stretch,
            Justify::FlexStart,
            Wrap::NoWrap,
            10.0,
            AnyView::new(TupleView::new(vec![
                header,
                AnyView::new(
                    AnyView::new(ScrollView::vertical(body).combine_accessibility(false))
                        .flex_with(1.0, 1.0, Some(0.0)),
                ),
            ])),
        )
        .padding_insets(EdgeInsets::new(16.0, 16.0, 16.0, 16.0))
        .fill_width()
        .fill_height()
        .semantic_background(SemanticColorRef::surface())
        .clipped(),
    )
}

fn settings_section(title: &str, rows: Vec<AnyView>) -> AnyView {
    AnyView::new(
        BoxView::new(
            FlexDirection::Column,
            Align::Stretch,
            Justify::FlexStart,
            Wrap::NoWrap,
            12.0,
            AnyView::new(TupleView::new(
                std::iter::once(section_label(title, 0.0))
                    .chain(rows.into_iter())
                    .collect(),
            )),
        )
        .padding_insets(EdgeInsets::new(10.0, 12.0, 10.0, 12.0))
        .fill_width()
        .corner_radius(10.0)
        .semantic_background(SemanticColorRef::surface_sunken())
        .semantic_border(SemanticColorRef::border(), 0.5),
    )
}

fn settings_toggle(title: &'static str, hint: &'static str, binding: cove_ui::core::Binding<bool>) -> AnyView {
    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            12.0,
            AnyView::new(views![
                AnyView::new(
                    Column::new(
                        HorizontalAlignment::Leading,
                        2.0,
                        AnyView::new(views!(
                            Text::new(title)
                                .semantic_font(SemanticFontRef::body())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Text::new(hint)
                                .line_limit(Some(2))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        )),
                    )
                    .frame_min_width(0.0)
                    .flex(1.0),
                ),
                AnyView::new(Toggle::new(binding)),
            ]),
        )
        .fill_width(),
    )
}

fn settings_info(title: &'static str, value: &'static str, hint: &'static str) -> AnyView {
    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            12.0,
            AnyView::new(views![
                Text::new(title)
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Spacer::new(0.0),
                Text::new(value)
                    .semantic_font(SemanticFontRef::label())
                    .semantic_foreground(SemanticColorRef::on_surface_variant()),
            ]),
        )
        .fill_width()
        .tooltip(hint),
    )
}

// ---------------------------------------------------------------------------
// shared bits
// ---------------------------------------------------------------------------

/// A runtime-length column: `views!` has a fixed arity, so an index of rows is
/// built through a flex box around a tuple view.
fn vstack(spacing: f32, children: Vec<AnyView>) -> AnyView {
    AnyView::new(BoxView::new(
        FlexDirection::Column,
        Align::Stretch,
        Justify::FlexStart,
        Wrap::NoWrap,
        spacing,
        AnyView::new(TupleView::new(children)),
    ))
}

fn section_label(title: &str, top: f32) -> AnyView {
    AnyView::new(
        AnyView::new(
            Text::new(title.to_owned())
                .line_limit(Some(1))
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        )
        .fill_width()
        .padding_insets(EdgeInsets::new(top, 14.0, 6.0, 0.0)),
    )
}

/// A sidebar row: glyph, name, and an optional count on the right edge — the
/// count is what makes "位置" a place index instead of a menu.
fn side_row(
    glyph: cove_ui::imaging::BundleImageResource,
    label: String,
    trailing: Option<String>,
    selected: bool,
    action: impl Fn() + 'static,
) -> AnyView {
    let text_color = if selected {
        SemanticColorRef::on_surface()
    } else {
        SemanticColorRef::on_surface_variant()
    };
    let font = if selected {
        SemanticFontRef::body_strong()
    } else {
        SemanticFontRef::body()
    };
    let glyph_color = if selected {
        SemanticColorRef::accent()
    } else {
        SemanticColorRef::on_surface_variant()
    };

    let trailing_view: AnyView = match trailing {
        Some(count) => AnyView::new(
            Text::new(count)
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(if selected {
                    SemanticColorRef::on_surface_variant()
                } else {
                    SemanticColorRef::on_surface_muted()
                }),
        ),
        None => AnyView::new(EmptyView),
    };

    let button = Button::new(
        Row::new(
            VerticalAlignment::Center,
            9.0,
            AnyView::new(views![
                AnyView::new(ButtonIcon::new(
                    ButtonIconSource::resource(glyph),
                    Some(15.0),
                    None,
                ))
                .semantic_foreground(glyph_color),
                Text::new(label)
                    .line_limit(Some(1))
                    .truncating_tail()
                    .semantic_font(font)
                    .semantic_foreground(text_color),
                Spacer::new(0.0),
                trailing_view,
            ]),
        )
        .fill_width(),
        action,
    )
    .button_style(GhostButtonStyle)
    .control_size(ControlSize::Small)
    .fill_width();

    let body = AnyView::new(button).frame(None, None, None, Some(32.0), None, None);
    let row = if selected {
        AnyView::new(body)
            .corner_radius(6.0)
            .semantic_background(SemanticColorRef::accent_muted())
    } else {
        body
    };

    AnyView::new(
        AnyView::new(row.fill_width())
            .padding_insets(EdgeInsets::new(1.0, 6.0, 1.0, 6.0))
            .fill_width(),
    )
}

/// A quiet text-only action with its glyph, for "添加文件夹…".
fn sub_row_icon(
    glyph: cove_ui::imaging::BundleImageResource,
    label: &str,
    action: impl Fn() + 'static,
) -> AnyView {
    let button = AnyView::new(
        Button::new(
            Row::new(
                VerticalAlignment::Center,
                9.0,
                AnyView::new(views![
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(glyph),
                        Some(15.0),
                        None,
                    ))
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    Text::new(label.to_owned())
                        .line_limit(Some(1))
                        .truncating_tail()
                        .semantic_font(SemanticFontRef::body())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    Spacer::new(0.0),
                ]),
            )
            .fill_width(),
            action,
        )
        .button_style(GhostButtonStyle)
        .control_size(ControlSize::Small)
        .fill_width(),
    )
    .frame(None, None, None, Some(32.0), None, None);

    AnyView::new(
        AnyView::new(button)
            .padding_insets(EdgeInsets::new(10.0, 6.0, 2.0, 6.0))
            .fill_width(),
    )
}

fn file_glyph(ext: &str, folder: bool) -> (cove_ui::imaging::BundleImageResource, SemanticColorRef) {
    if folder {
        return (file::folder(), SemanticColorRef::on_surface_variant());
    }
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "heic" | "bmp" => {
            (media::image(), SemanticColorRef::accent())
        }
        "mp3" | "wav" | "flac" | "aac" | "m4a" => (media::music(), SemanticColorRef::accent()),
        "mp4" | "mov" | "mkv" | "avi" | "webm" => (media::video(), SemanticColorRef::accent()),
        _ => (file::file(), SemanticColorRef::on_surface_variant()),
    }
}

fn compare_text(left: &str, right: &str) -> ComparisonResult {
    compare_ordering(left.cmp(right))
}

fn compare_number(left: u64, right: u64) -> ComparisonResult {
    compare_ordering(left.cmp(&right))
}

fn compare_ordering(ordering: Ordering) -> ComparisonResult {
    match ordering {
        Ordering::Less => ComparisonResult::Ascending,
        Ordering::Greater => ComparisonResult::Descending,
        Ordering::Equal => ComparisonResult::Same,
    }
}
