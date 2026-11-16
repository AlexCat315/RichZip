//! One window, in three places at once: a location index down the left edge,
//! the packages living in that location, and — once one is opened — the
//! archive's own contents. The sidebar answers "where am I", never "what
//! happened"; progress belongs to the package it is unpacking.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use cove_ui::compose::adaptive::SurfaceEnvironment;
use cove_ui::compose::modifiers::layout::EdgeInsets;
use cove_ui::compose::modifiers::style::AppearanceExt;
use cove_ui::compose::prelude::*;
use cove_ui::compose::primitives::controls::{RadioGroup, RadioOption, Toggle};
use cove_ui::compose::primitives::text::text_field::TextField;
use cove_ui::compose::primitives::data::table::{ColumnVisibility, ComparisonResult};
use cove_ui::compose::primitives::feedback::ProgressView;
use cove_ui::compose::primitives::layout::{
    ScrollView, SplitAxis, SplitConstraints, SplitView,
};
use cove_ui::compose::primitives::media::ui_icon_catalog::{action, file, media, navigation};
use cove_ui::compose::primitives::media::{Image, ImageContentMode};
use cove_ui::compose::primitives::overlays::portal::LayerRoot;
use cove_ui::compose::primitives::overlays::{Modal, ModalOptions};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, GhostButtonStyle, PlainButtonStyle,
};
use cove_ui::composition::{
    BoxView, DynamicField, EmptyView, HorizontalAlignment, Scope, Spacer, TupleView,
    VerticalAlignment, views,
};
use cove_ui::core::scene::{Align, FlexDirection, Justify, Wrap};
use cove_ui::core::Color;

use super::dashed::DashedBorder;
use crate::browser::FileItem;
use crate::model::{
    AppModel, LoadedArchive, MultiPreview, PackageItem, Phase, PreviewCard, PreviewContent,
    PreviewState, Tone,
};
use mugenzip_core::{CompressionLevel, CreateFormat};
use crate::theme::{self, ThemeMode};
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

/// The inspector is a fixed lease beside the list: 304 pt, 12 pt air, and
/// never on top of content. It is the HTML design's measured value, not a
/// percentage, so resizing the window never makes the card jitter.
const INSPECTOR_WIDTH: f32 = 304.0;
const INSPECTOR_GAP: f32 = 12.0;

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
            .maximum_files(20)
            .shows_chrome(false)
            .is_targeted(model.drop_targeted.binding())
            .on_targeted(move |is_targeted| targeted.drop_targeted.set(is_targeted))
            .fill(),
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
            .options(options.clone())
            .debug_name("settings-sheet"),
        );

        let mut create_options = ModalOptions::new();
        create_options.geometry.width = 520.0;
        create_options.geometry.height = 600.0;
        let create = AnyView::new(
            Modal::new(
                model.create_open.binding(),
                create_panel(model.clone()),
            )
            .options(create_options)
            .debug_name("create-sheet"),
        );

        // A PDF page is 612 × 792 points. At the inspector's 272 pt width its
        // 12 pt body text lands around five pixels, which is not a preview —
        // so the inspector keeps the whole-page fit and "放大" hands the page
        // to a reader that draws it at its own size, scrolling vertically.
        let reader_model = model.clone();
        let reader_get = {
            let model = model.clone();
            move || {
                matches!(
                    model.preview.get(),
                    PreviewState::Single(card) if card.pdf_zoomed
                )
            }
        };
        let reader_set = {
            let model = reader_model.clone();
            move |open: bool| {
                if !open {
                    model.close_pdf_reader();
                }
            }
        };
        let reader_open = Binding::new(reader_get, reader_set);
        let mut reader_options = ModalOptions::new();
        reader_options.geometry.width = 900.0;
        reader_options.geometry.height = 760.0;
        let reader = AnyView::new(
            Modal::new(reader_open, pdf_reader_panel(model.clone()))
                .options(reader_options)
                .debug_name("pdf-reader"),
        );

        // One portal layer, one sheet: stacking two modal nodes makes the
        // second one paint under the workspace. The sheets are mutually
        // exclusive, so pick whichever is open for this frame.
        let reader_visible = matches!(
            model.preview.get(),
            PreviewState::Single(card) if card.pdf_zoomed
        );
        let sheet = if reader_visible {
            reader
        } else if model.create_open.get() || !model.settings_open.get() {
            create
        } else {
            settings
        };

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
                    AnyView::new(TupleView::new(vec![workspace_view, sheet])),
                )
                .fill(),
            )))
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
        .modifier(DashedBorder {
            color: theme::drop_stroke(),
            width: 1.0,
            radius: 12.0,
        })
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
    let busy = matches!(model.phase.get(), Phase::Extracting | Phase::Compressing);
    let progress: AnyView = if busy {
        progress_strip(model.clone())
    } else {
        AnyView::new(EmptyView)
    };

    let inspector_visible = !matches!(model.preview.get(), PreviewState::Hidden);
    let left = AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views![
                crumb_bar(model.clone()),
                list_area(model.clone()),
            ]),
        )
        .flex(1.0)
        .fill_height(),
    );
    let inspector = AnyView::new(if inspector_visible {
        inspector_panel(model.clone())
    } else {
        AnyView::new(EmptyView)
    })
    .padding_insets(EdgeInsets::new(
        0.0,
        0.0,
        0.0,
        if inspector_visible { GUTTER } else { 0.0 },
    ))
    .frame_width(if inspector_visible { INSPECTOR_WIDTH } else { 0.0 })
    .fill_height();
    let body = Row::new(
        VerticalAlignment::Top,
        if inspector_visible { INSPECTOR_GAP } else { 0.0 },
        AnyView::new(views![left, inspector]),
    )
    .flex(1.0);

    AnyView::new(
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                0.0,
                AnyView::new(views![
                    toolbar_package(model.clone(), safe_top),
                    progress,
                    body,
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
    let compress = model.clone();
    let compress_action = toolbar_action(
        action::plus(),
        "压缩",
        !model.is_busy(),
        false,
        move || compress.open_create(),
    );

    let tools = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            14.0,
            AnyView::new(views![
                search_field(model.clone()),
                open_action,
                compress_action,
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
    archive.meta_line(model.show_system_files.get())
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
    let create = model.clone();
    let mut entries = vec![
        MenuEntry::Item(
            MenuItem::new(4, "打开压缩包…", move || open.open_dialog()).shortcut("⌘O"),
        ),
        MenuEntry::Item(MenuItem::new(12, "新建压缩包…", move || create.open_create())),
    ];
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
    let compressing = model.phase.get() == Phase::Compressing;
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
                        Text::new(if compressing { "压缩中" } else { "解压中" })
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
                        .accessibility_text("取消当前任务")
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

// ---------------------------------------------------------------------------
// inspector: selection in, one card out
// ---------------------------------------------------------------------------

fn inspector_panel(model: Rc<AppModel>) -> AnyView {
    match model.preview.get() {
        PreviewState::Hidden => AnyView::new(EmptyView),
        PreviewState::Single(card) => inspector_card(model, &card),
        PreviewState::Multi(multi) => inspector_multi(model, &multi),
    }
}

fn inspector_shell(body: AnyView, footer: AnyView, debug: &str) -> AnyView {
    AnyView::new(
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                0.0,
                AnyView::new(views![body, footer]),
            )
            .semantic_background(SemanticColorRef::surface())
            .semantic_border(SemanticColorRef::border(), 1.0)
            .corner_radius(10.0)
            .clipped()
            .fill_width()
            .fill_height(),
        )
        .debug_name(debug),
    )
}

fn preview_header(card: &PreviewCard) -> AnyView {
    let subtitle = if card.extension.is_empty() {
        card.size.clone()
    } else {
        format!("{} · {}", card.extension.to_uppercase(), card.size)
    };
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            5.0,
            AnyView::new(views![
                Row::new(
                    VerticalAlignment::Center,
                    6.0,
                    AnyView::new(views![
                        Text::new("•")
                            .semantic_font(SemanticFontRef::body())
                            .semantic_foreground(SemanticColorRef::accent()),
                        Text::new(card.label())
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::accent()),
                    ]),
                ),
                Text::new(card.name.clone())
                    .line_limit(Some(2))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::body_strong())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Text::new(subtitle)
                    .line_limit(Some(1))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            ]),
        )
        .padding_insets(EdgeInsets::new(14.0, 16.0, 12.0, 16.0))
        .fill_width(),
    )
}

fn metadata_line(label: &str, value: &str) -> AnyView {
    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            0.0,
            AnyView::new(views![
                Text::new(label.to_owned())
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
                Spacer::new(0.0),
                Text::new(value.to_owned())
                    .line_limit(Some(1))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface()),
            ]),
        )
        .fill_width(),
    )
}

fn preview_footer(card: &PreviewCard) -> AnyView {
    let mut rows = vec![metadata_line("大小", &card.size)];
    if !card.modified.trim().is_empty() {
        rows.push(metadata_line("修改时间", &card.modified));
    }
    rows.push(
        AnyView::new(
            Row::new(
                VerticalAlignment::Center,
                6.0,
                AnyView::new(views![
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(action::external_link()),
                        Some(11.0),
                        None,
                    ))
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    Text::new("双击这一行，交给系统默认应用")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                ]),
            )
            .padding_insets(EdgeInsets::new(7.0, 0.0, 0.0, 0.0)),
        ),
    );
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            4.0,
            AnyView::new(TupleView::new(rows)),
        )
        .padding_insets(EdgeInsets::new(10.0, 16.0, 12.0, 16.0))
        .fill_width(),
    )
}

fn inspector_card(model: Rc<AppModel>, card: &PreviewCard) -> AnyView {
    let body = AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views![
                preview_header(card),
                AnyView::new(Divider::horizontal().thickness(1.0)),
                AnyView::new(ScrollView::vertical(preview_body(model.clone(), card)))
                    .flex(1.0)
                    .fill_width(),
                AnyView::new(Divider::horizontal().thickness(1.0)),
                preview_footer(card),
            ]),
        )
        .fill_width()
        .fill_height(),
    );
    inspector_shell(
        body,
        AnyView::new(EmptyView),
        "preview-inspector",
    )
}

fn preview_body(model: Rc<AppModel>, card: &PreviewCard) -> AnyView {
    match &card.content {
        PreviewContent::Loading => AnyView::new(
            Column::new(
                HorizontalAlignment::Center,
                10.0,
                AnyView::new(views![
                    Spinner::new().size(18.0),
                    Text::new("正在抽取这一条…")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                ]),
            )
            .padding_insets(EdgeInsets::new(34.0, 16.0, 34.0, 16.0))
            .fill_width(),
        ),
        PreviewContent::Image(path) => media_body(model, card, path, false),
        PreviewContent::Pdf(path) => media_body(model, card, path, true),
        PreviewContent::Text { content, truncated } => {
            let items: Vec<AnyView> = vec![
                AnyView::new(
                    AnyView::new(text_document(content, &card.extension, *truncated))
                        .padding_insets(EdgeInsets::new(16.0, 16.0, 0.0, 16.0)),
                ),
                AnyView::new(related_section(model.clone(), &card.id)),
            ];
            AnyView::new(
                Column::new(
                    HorizontalAlignment::Leading,
                    0.0,
                    AnyView::new(TupleView::new(items)),
                )
                .fill_width(),
            )
        }
        PreviewContent::Note { message, error } => note_body(model, card, message, *error),
    }
}

fn media_body(
    model: Rc<AppModel>,
    card: &PreviewCard,
    path: &PathBuf,
    pdf: bool,
) -> AnyView {
    // A 612 × 792 page at 272 pt leaves body text about five pixels tall. The
    // inspector therefore shows an honest whole-page fit; "放大阅读" opens the
    // reader sheet, where the page is drawn at its own point size.
    const VIEWPORT: Size = Size::new(272.0, 312.0);
    let page = AnyView::new(
        BoxView::new(
            FlexDirection::Column,
            Align::FlexStart,
            Justify::FlexStart,
            Wrap::NoWrap,
            0.0,
            AnyView::new(
                AnyView::new(
                    Image::from_file(path, VIEWPORT).content_mode(if pdf {
                        ImageContentMode::Fit
                    } else {
                        ImageContentMode::Fill
                    }),
                )
                .fill_width()
                .fill_height(),
            ),
        )
        .frame_size(VIEWPORT.width, VIEWPORT.height)
        .frame_min_width(VIEWPORT.width)
        .shrink(0.0),
    );
    let framed = AnyView::new(page)
        .frame_width(VIEWPORT.width)
        .frame_height(VIEWPORT.height)
        .corner_radius(8.0)
        .semantic_border(SemanticColorRef::border(), 0.5)
        .clipped();

    let caption = if pdf {
        let toggle = model.clone();
        AnyView::new(
            Row::new(
                VerticalAlignment::Center,
                8.0,
                AnyView::new(views![
                    Text::new("第 1 页 · 整页缩略".to_owned())
                        .line_limit(Some(1))
                        .truncating_tail()
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted())
                        .frame_min_width(0.0)
                        .flex(1.0),
                    Button::new(
                        Text::new("放大阅读".to_owned())
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        move || toggle.toggle_pdf_zoom(),
                    )
                    .accessibility_text("打开 PDF 阅读面板")
                    .button_style(GhostButtonStyle)
                    .control_size(ControlSize::Small),
                ]),
            )
            .fill_width(),
        )
    } else {
        AnyView::new(
            Text::new("双击交给系统打开原文件".to_owned())
                .line_limit(Some(1))
                .truncating_tail()
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        )
    };

    let items = vec![
        AnyView::new(
            Column::new(
                HorizontalAlignment::Center,
                8.0,
                AnyView::new(views![framed, caption]),
            )
            .padding_insets(EdgeInsets::new(12.0, 12.0, 8.0, 12.0))
            .fill_width(),
        ),
        related_section(model, &card.id),
    ];
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(TupleView::new(items)),
        )
        .semantic_background(SemanticColorRef::surface_sunken())
        .fill_width(),
    )
}

fn note_body(
    model: Rc<AppModel>,
    card: &PreviewCard,
    message: &str,
    error: bool,
) -> AnyView {
    let tint = if error {
        SemanticColorRef::error()
    } else {
        SemanticColorRef::on_surface_muted()
    };
    let glyph = preview_glyph(card);
    let items = vec![
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                10.0,
                AnyView::new(views![
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(glyph),
                        Some(34.0),
                        None,
                    ))
                    .semantic_foreground(tint),
                    Text::new(card.label().to_owned())
                        .semantic_font(SemanticFontRef::body_strong())
                        .semantic_foreground(SemanticColorRef::on_surface()),
                    Text::new(message.to_owned())
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                ]),
            )
            .padding_insets(EdgeInsets::new(24.0, 18.0, 24.0, 18.0))
            .fill_width(),
        ),
        related_section(model, &card.id),
    ];
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(TupleView::new(items)),
        )
        .fill_width(),
    )
}

fn preview_glyph(card: &PreviewCard) -> cove_ui::imaging::BundleImageResource {
    if matches!(card.content, PreviewContent::Note { error: true, .. }) {
        return file::file();
    }
    match card.kind {
        crate::preview::PreviewKind::Image => media::image(),
        crate::preview::PreviewKind::Pdf => file::file_text(),
        crate::preview::PreviewKind::Text => file::file_text(),
        crate::preview::PreviewKind::Unsupported => file::archive(),
    }
}

fn text_document(content: &str, extension: &str, truncated: bool) -> AnyView {
    let mono = matches!(
        extension.to_ascii_lowercase().as_str(),
        "json" | "toml" | "yaml" | "yml" | "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "rb"
            | "go" | "java" | "kt" | "swift" | "c" | "h" | "cpp" | "hpp" | "m" | "mm"
            | "sh" | "zsh" | "bash" | "fish" | "ps1" | "bat" | "cmd" | "css" | "scss"
            | "less" | "sql"
    );
    let mut items: Vec<AnyView> = Vec::new();
    // A text preview is read, not audited. Stop at a screenful so a 512 KB
    // log never builds ten thousand text nodes.
    for line in content.lines().take(600) {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            items.push(AnyView::new(Spacer::new(6.0)));
            continue;
        }
        if !mono && let Some(heading) = trimmed.strip_prefix("# ") {
            items.push(AnyView::new(
                Text::new(strip_inline(heading))
                    .semantic_font(SemanticFontRef::headline())
                    .semantic_foreground(SemanticColorRef::on_surface()),
            ));
            items.push(AnyView::new(Spacer::new(2.0)));
            continue;
        }
        if !mono && let Some(heading) = trimmed.strip_prefix("## ") {
            items.push(AnyView::new(
                Text::new(strip_inline(heading))
                    .semantic_font(SemanticFontRef::body_strong())
                    .semantic_foreground(SemanticColorRef::on_surface()),
            ));
            continue;
        }
        let text = if !mono
            && (trimmed.starts_with("- ") || trimmed.starts_with("* "))
        {
            format!("·  {}", strip_inline(&trimmed[2..]))
        } else {
            strip_inline(trimmed)
        };
        items.push(AnyView::new(
            Text::new(text)
                .semantic_font(if mono {
                    SemanticFontRef::mono()
                } else {
                    SemanticFontRef::body()
                })
                .semantic_foreground(SemanticColorRef::on_surface()),
        ));
    }
    if truncated || content.lines().count() > 600 {
        items.push(AnyView::new(Spacer::new(8.0)));
        items.push(AnyView::new(
            Text::new("内容较长，这里只显示开头部分。双击交给系统查看完整文件。")
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        ));
    }
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            3.0,
            AnyView::new(TupleView::new(items)),
        )
        .fill_width(),
    )
}

fn strip_inline(text: &str) -> String {
    text.replace("**", "")
        .replace("`", "")
        .replace("__", "")
        .trim()
        .to_owned()
}

fn related_section(model: Rc<AppModel>, current_id: &str) -> AnyView {
    let siblings: Vec<FileItem> = model
        .current_items()
        .into_iter()
        .filter(|item| !item.folder && item.id != current_id)
        .take(3)
        .collect();
    if siblings.is_empty() {
        return AnyView::new(EmptyView);
    }
    let count = model
        .current_items()
        .into_iter()
        .filter(|item| !item.folder && item.id != current_id)
        .count();
    let mut rows: Vec<AnyView> = vec![AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            6.0,
            AnyView::new(views![
                AnyView::new(EmptyView)
                    .frame_height(1.0)
                    .fill_width()
                    .semantic_background(SemanticColorRef::divider()),
                Text::new(format!("同目录 · {count} 个其他文件"))
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            ]),
        )
        .padding_insets(EdgeInsets::new(12.0, 16.0, 2.0, 16.0))
        .fill_width(),
    )];
    for sibling in siblings {
        let id = sibling.id.clone();
        let (glyph, tint) = file_glyph(&sibling.ext, false);
        let pick = model.clone();
        rows.push(AnyView::new(
            Button::new(
                AnyView::new(
                    Row::new(
                        VerticalAlignment::Center,
                        8.0,
                        AnyView::new(views![
                            AnyView::new(ButtonIcon::new(
                                ButtonIconSource::resource(glyph),
                                Some(13.0),
                                None,
                            ))
                            .semantic_foreground(tint),
                            Text::new(sibling.name.clone())
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Spacer::new(0.0),
                            Text::new(sibling.size.clone())
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        ]),
                    )
                    .fill_width(),
                ),
                move || pick.select_preview_id(id.clone()),
            )
            .button_style(PlainButtonStyle)
            .fill_width()
            .padding_insets(EdgeInsets::new(5.0, 16.0, 5.0, 16.0)),
        ));
    }
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(TupleView::new(rows)),
        )
        .fill_width(),
    )
}

fn inspector_multi(model: Rc<AppModel>, multi: &MultiPreview) -> AnyView {
    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    for row in &multi.items {
        let ext = PathBuf::from(&row.name)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_uppercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "其他".to_owned());
        *types.entry(ext).or_insert(0) += 1;
    }
    let mut rows: Vec<AnyView> = vec![];
    for (ext, count) in types {
        rows.push(AnyView::new(
            Row::new(
                VerticalAlignment::Center,
                8.0,
                AnyView::new(views![
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(file::file()),
                        Some(13.0),
                        None,
                    ))
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    Text::new(ext)
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface()),
                    Spacer::new(0.0),
                    Text::new(format!("{count} 个"))
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                ]),
            )
            .padding_insets(EdgeInsets::new(5.0, 16.0, 5.0, 16.0))
            .fill_width(),
        ));
    }
    let body = AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views![
                AnyView::new(
                    Column::new(
                        HorizontalAlignment::Leading,
                        5.0,
                        AnyView::new(views![
                            Row::new(
                                VerticalAlignment::Center,
                                6.0,
                                AnyView::new(views![
                                    Text::new("•")
                                        .semantic_font(SemanticFontRef::body())
                                        .semantic_foreground(SemanticColorRef::accent()),
                                    Text::new("多选汇总")
                                        .semantic_font(SemanticFontRef::caption())
                                        .semantic_foreground(SemanticColorRef::accent()),
                                ]),
                            ),
                            Text::new(format!("已选择 {} 个项目", multi.items.len()))
                                .semantic_font(SemanticFontRef::body_strong())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Text::new(format!("共 {}", format_size(multi.bytes)))
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        ]),
                    )
                    .padding_insets(EdgeInsets::new(14.0, 16.0, 12.0, 16.0))
                    .fill_width(),
                ),
                AnyView::new(Divider::horizontal().thickness(1.0)),
                AnyView::new(TupleView::new(rows)),
                AnyView::new(
                    Text::new("多选时不抽取单条预览。选择一行即可查看内容，或直接解压这些项目。")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                )
                .padding_insets(EdgeInsets::new(12.0, 16.0, 16.0, 16.0)),
            ]),
        )
        .fill_width(),
    );
    let clear = model.clone();
    let footer = AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views![
                AnyView::new(Divider::horizontal().thickness(1.0)),
                AnyView::new(
                    Button::new(
                        AnyView::new(
                            Text::new("清除选择")
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                        ),
                        move || clear.change_selection(BTreeSet::new()),
                    )
                    .button_style(PlainButtonStyle),
                )
                .padding_insets(EdgeInsets::new(10.0, 16.0, 10.0, 16.0)),
            ]),
        )
        .fill_width(),
    );
    inspector_shell(
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                0.0,
                AnyView::new(views![body, footer]),
            )
            .fill_width()
            .fill_height(),
        ),
        AnyView::new(EmptyView),
        "preview-multi",
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
    if matches!(model.phase.get(), Phase::Extracting | Phase::Compressing) {
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
    // When the inspector is open the table's outer gutter moves into the
    // inspector gap, so the card keeps its 12 pt air instead of 32.
    let right = if matches!(model.preview.get(), PreviewState::Hidden) {
        GUTTER
    } else {
        0.0
    };
    AnyView::new(
        Column::new(HorizontalAlignment::Leading, 0.0, AnyView::new(file_table(model)))
            .padding_insets(EdgeInsets::new(4.0, GUTTER, GUTTER, right))
            .flex(1.0)
            .fill_width(),
    )
}

fn file_table(model: Rc<AppModel>) -> AnyView {
    let searching = !model.search.get().string_value().trim().is_empty();
    let mut options = TableOptions::default();
    options.selection = Some(Selection::multiple(model.selection_binding()));
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
// create sheet
// ---------------------------------------------------------------------------

/// "新建压缩包" — the same shape as the settings sheet: one header, grouped
/// rows, a running job pinned above the legend. Sources are gathered here
/// rather than in the browser so a file from anywhere can join the archive.
fn create_panel(model: Rc<AppModel>) -> AnyView {
    let close = model.clone();
    let start = model.clone();
    let busy = model.is_busy();
    let count = model.create_source_count();

    let header = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            8.0,
            AnyView::new(views![
                Text::new("新建压缩包")
                    .semantic_font(SemanticFontRef::title())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Spacer::new(0.0),
                Button::new(
                    Text::new("取消")
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(SemanticColorRef::on_surface_variant()),
                    move || close.close_create(),
                )
                .enabled(!busy)
                .accessibility_text("关闭新建压缩包")
                .button_style(PlainButtonStyle)
                .control_size(ControlSize::Small),
                Button::new(
                    Text::new("压缩")
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(SemanticColorRef::accent()),
                    move || start.create_now(),
                )
                .enabled(!busy && count > 0)
                .accessibility_text("开始压缩")
                .tooltip("打包到所选位置")
                .button_style(PlainButtonStyle)
                .control_size(ControlSize::Small),
            ]),
        )
        .fill_width(),
    );

    let sources = model.create_sources.get();
    let mut rows: Vec<AnyView> = Vec::new();
    if sources.is_empty() {
        rows.push(AnyView::new(
            Text::new("还没有内容。拖入文件或文件夹，或从下面添加。")
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        ));
    } else {
        for (index, path) in sources.iter().enumerate() {
            rows.push(create_source_row(model.clone(), index, path));
        }
    }
    let add_file = model.clone();
    let add_folder = model.clone();
    rows.push(AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            8.0,
            AnyView::new(views![
                Button::new(
                    Text::new("添加文件…")
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(SemanticColorRef::on_surface()),
                    move || add_file.add_create_files(),
                )
                .enabled(!busy)
                .button_style(GhostButtonStyle)
                .control_size(ControlSize::Small),
                Button::new(
                    Text::new("添加文件夹…")
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(SemanticColorRef::on_surface()),
                    move || add_folder.add_create_folder(),
                )
                .enabled(!busy)
                .button_style(GhostButtonStyle)
                .control_size(ControlSize::Small),
            ]),
        )
        .padding_insets(EdgeInsets::new(6.0, 0.0, 0.0, 0.0)),
    ));
    let count_line = if count == 0 {
        "空".to_owned()
    } else {
        format!("{count} 项")
    };
    let source_section = settings_section(&format!("内容 · {count_line}"), rows);

    let current_format = model.create_format.get();
    let mut format_chips: Vec<AnyView> = Vec::new();
    for format in CreateFormat::ALL {
        let model_for_chip = model.clone();
        format_chips.push(create_choice(
            format.label(),
            current_format == format,
            move || model_for_chip.set_create_format(format),
        ));
    }
    let format_note = AnyView::new(
        Text::new(current_format.note())
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
    );
    let format_section = settings_section(
        "格式",
        vec![
            AnyView::new(Row::new(VerticalAlignment::Center, 6.0, AnyView::new(format_chips))),
            format_note,
        ],
    );

    let current_level = model.create_level.get();
    let mut level_chips: Vec<AnyView> = Vec::new();
    for level in CompressionLevel::ALL {
        let model_for_chip = model.clone();
        level_chips.push(create_choice(
            level.label(),
            current_level == level,
            move || model_for_chip.set_create_level(level),
        ));
    }
    let level_section = settings_section(
        "压缩级别",
        vec![AnyView::new(Row::new(
            VerticalAlignment::Center,
            6.0,
            AnyView::new(level_chips),
        ))],
    );

    let extension = format!(".{}", model.create_format.get().extension());
    let name_field = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views![
                Text::new("文件名")
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                AnyView::new(TextField::new("归档", model.create_name.binding()))
                    .frame_min_width(0.0)
                    .flex(1.0),
                Text::new(extension)
                    .semantic_font(SemanticFontRef::mono())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            ]),
        )
        .fill_width(),
    );

    let choose = model.clone();
    let destination_row = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            12.0,
            AnyView::new(views![
                AnyView::new(
                    Column::new(
                        HorizontalAlignment::Leading,
                        2.0,
                        AnyView::new(views!(
                            Text::new("保存到")
                                .semantic_font(SemanticFontRef::body())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Text::new(model.create_destination_name())
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        )),
                    )
                    .frame_min_width(0.0)
                    .flex(1.0),
                ),
                Button::new(
                    Text::new("更改…")
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(SemanticColorRef::on_surface()),
                    move || choose.choose_create_destination(),
                )
                .enabled(!busy)
                .button_style(GhostButtonStyle)
                .control_size(ControlSize::Small),
            ]),
        )
        .fill_width(),
    );
    let naming_section = settings_section("命名与位置", vec![name_field, destination_row]);

    let job: AnyView = if busy {
        let progress = model.progress.get();
        let cancel = model.clone();
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                4.0,
                AnyView::new(views!(
                    Row::new(
                        VerticalAlignment::Center,
                        10.0,
                        AnyView::new(views!(
                            Text::new("压缩中")
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Text::new(progress.label.clone())
                                .semantic_font(SemanticFontRef::mono())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                            Text::new(progress.current_file.clone())
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted())
                                .frame_min_width(0.0)
                                .flex(1.0),
                            Button::new(
                                Text::new("取消")
                                    .semantic_font(SemanticFontRef::caption())
                                    .semantic_foreground(SemanticColorRef::on_surface()),
                                move || cancel.cancel_extract(),
                            )
                            .accessibility_text("取消压缩")
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
            .padding_insets(EdgeInsets::new(8.0, 12.0, 8.0, 12.0))
            .fill_width()
            .corner_radius(10.0)
            .semantic_background(SemanticColorRef::surface_sunken())
            .semantic_border(SemanticColorRef::border(), 0.5),
        )
    } else {
        AnyView::new(EmptyView)
    };

    let legend = AnyView::new(
        Text::new("同名不会覆盖：重名自动加序号；取消或失败会删除临时文件。")
            .line_limit(Some(2))
            .truncating_tail()
            .semantic_font(SemanticFontRef::caption())
            .semantic_foreground(SemanticColorRef::on_surface_muted()),
    );

    let body = AnyView::new(vstack(
        8.0,
        vec![source_section, format_section, level_section, naming_section, legend],
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
                job,
                AnyView::new(
                    AnyView::new(ScrollView::vertical(body).combine_accessibility(false))
                        .flex_with(1.0, 1.0, Some(0.0)),
                ),
            ])),
        )
        .padding_insets(EdgeInsets::new(16.0, 16.0, 16.0, 16.0))
        .fill_width()
        .fill_height()
        .semantic_background(SemanticColorRef::surface_floating())
        .clipped(),
    )
}

/// One selectable label. Selected reads as accent ink on the sunken surface;
/// unselected stays quiet until hovered, like every other secondary control.
fn create_choice(label: &'static str, selected: bool, action: impl Fn() + 'static) -> AnyView {
    let ink = if selected {
        SemanticColorRef::accent()
    } else {
        SemanticColorRef::on_surface_variant()
    };
    AnyView::new(
        Button::new(
            AnyView::new(
                Text::new(label)
                    .semantic_font(SemanticFontRef::label())
                    .semantic_foreground(ink),
            )
            .padding_insets(EdgeInsets::new(5.0, 10.0, 5.0, 10.0))
            .corner_radius(7.0)
            .semantic_background(if selected {
                SemanticColorRef::surface()
            } else {
                SemanticColorRef::surface_sunken()
            })
            .semantic_border(
                if selected {
                    SemanticColorRef::accent()
                } else {
                    SemanticColorRef::border()
                },
                0.5,
            ),
            action,
        )
        .button_style(PlainButtonStyle)
        .control_size(ControlSize::Small),
    )
}

fn create_source_row(model: Rc<AppModel>, index: usize, path: &std::path::Path) -> AnyView {
    let remove = model.clone();
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let parent = path
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views![
                AnyView::new(ButtonIcon::new(
                    ButtonIconSource::resource(file::archive()),
                    Some(14.0),
                    None,
                ))
                .semantic_foreground(SemanticColorRef::on_surface_variant()),
                AnyView::new(
                    Column::new(
                        HorizontalAlignment::Leading,
                        2.0,
                        AnyView::new(views!(
                            Text::new(name)
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::body())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Text::new(parent)
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        )),
                    )
                    .frame_min_width(0.0)
                    .flex(1.0),
                ),
                Button::new(
                    Text::new("移除")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    move || remove.remove_create_source(index),
                )
                .accessibility_text("从压缩内容中移除")
                .button_style(PlainButtonStyle)
                .control_size(ControlSize::Small),
            ]),
        )
        .padding_insets(EdgeInsets::new(5.0, 2.0, 5.0, 2.0))
        .fill_width(),
    )
}

// ---------------------------------------------------------------------------
// settings sheet
// ---------------------------------------------------------------------------

/// A panel, not a second window: a title, grouped rows, and room for the
/// settings that do not exist yet. Every control here is real; the groups that
/// are still empty say so instead of offering switches that do nothing.
/// The page at a size that can actually be read. A 612 × 792 page drawn at
/// 820 pt is about 1.34×, so 12 pt body copy lands near 16 px instead of the
/// five pixels it gets when the whole page is squeezed into the 272 pt
/// inspector.
fn pdf_reader_panel(model: Rc<AppModel>) -> AnyView {
    const PAGE: Size = Size::new(840.0, 1088.0);
    const READ_VIEWPORT: Size = Size::new(840.0, 600.0);

    let card = match model.preview.get() {
        PreviewState::Single(card) => Some(card),
        _ => None,
    };
    let path = card.as_ref().and_then(|card| match &card.content {
        PreviewContent::Pdf(path) => Some(path.clone()),
        _ => None,
    });
    let name = card
        .as_ref()
        .map(|card| card.name.clone())
        .unwrap_or_else(|| "PDF".to_owned());

    let close = model.clone();
    let header = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            8.0,
            AnyView::new(views![
                Column::new(
                    HorizontalAlignment::Leading,
                    2.0,
                    AnyView::new(views![
                        Text::new(name)
                            .line_limit(Some(1))
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::title())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        Text::new("第 1 页 · 适宽显示".to_owned())
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    ]),
                )
                .frame_min_width(0.0)
                .flex(1.0),
                Button::new(
                    Text::new("关闭".to_owned())
                        .semantic_font(SemanticFontRef::label())
                        .semantic_foreground(SemanticColorRef::accent()),
                    move || close.close_pdf_reader(),
                )
                .accessibility_text("关闭 PDF 阅读面板")
                .button_style(PlainButtonStyle)
                .control_size(ControlSize::Small),
            ]),
        )
        .fill_width(),
    );

    let body = match path {
        Some(path) => {
            let page = AnyView::new(
                BoxView::new(
                    FlexDirection::Column,
                    Align::FlexStart,
                    Justify::FlexStart,
                    Wrap::NoWrap,
                    0.0,
                    AnyView::new(
                        AnyView::new(
                            Image::from_file(&path, PAGE).content_mode(ImageContentMode::Fill),
                        )
                        .fill_width()
                        .fill_height(),
                    ),
                )
                .frame_size(PAGE.width, PAGE.height)
                .frame_min_width(PAGE.width)
                .frame(None, None, None, Some(PAGE.height), None, None)
                .shrink(0.0),
            );
            AnyView::new(
                Column::new(
                    HorizontalAlignment::Leading,
                    6.0,
                    AnyView::new(views![
                        AnyView::new(
                            ScrollView::vertical(AnyView::new(page))
                                .frame_width(READ_VIEWPORT.width)
                                .frame_height(READ_VIEWPORT.height)
                                .semantic_background(SemanticColorRef::surface_sunken())
                                .corner_radius(8.0)
                                .semantic_border(SemanticColorRef::border(), 0.5)
                                .clipped(),
                        ),
                        Text::new("上下滚动查看整页".to_owned())
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    ]),
                )
                .fill_width(),
            )
        }
        None => AnyView::new(
            Text::new("这一项不是 PDF，或预览已结束。".to_owned())
                .semantic_font(SemanticFontRef::body())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        ),
    };

    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            14.0,
            AnyView::new(views![header, body]),
        )
        .padding_insets(EdgeInsets::all(20.0)),
    )
}

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
            settings_toggle(
                "显示系统文件",
                "关闭时列表、解压、压缩都会跳过这些文件",
                model.show_system_files_binding(),
            ),
        ],
    );

    let appearance_section = settings_section(
        "外观",
        vec![
            settings_theme_row(model.clone()),
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

    let default_format = model.default_create_format.get();
    let mut format_chips: Vec<AnyView> = Vec::new();
    for format in CreateFormat::ALL {
        let model_for_chip = model.clone();
        format_chips.push(create_choice(
            format.label(),
            default_format == format,
            move || model_for_chip.set_default_create_format(format),
        ));
    }
    let default_level = model.default_create_level.get();
    let mut level_chips: Vec<AnyView> = Vec::new();
    for level in CompressionLevel::ALL {
        let model_for_chip = model.clone();
        level_chips.push(create_choice(
            level.label(),
            default_level == level,
            move || model_for_chip.set_default_create_level(level),
        ));
    }
    let compress_section = settings_section(
        "压缩默认值",
        vec![
            settings_choices("默认格式", "新任务和右键「压缩到此处」都从这里起步", format_chips),
            settings_choices("默认级别", "面板里仍可按本次任务临时改，不改这里", level_chips),
        ],
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
        // Match the Modal panel's own surface so the inner sheet never shows a
        // second, differently coloured backing at the bottom edge.
        .semantic_background(SemanticColorRef::surface_floating())
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

/// A title, a hint, and a row of chips: the shape settings groups share for
/// choices that are not switches.
fn settings_choices(title: &'static str, hint: &'static str, chips: Vec<AnyView>) -> AnyView {
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            5.0,
            AnyView::new(views![
                Text::new(title)
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Text::new(hint)
                    .line_limit(Some(2))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
                AnyView::new(Row::new(
                    VerticalAlignment::Center,
                    6.0,
                    AnyView::new(chips),
                )),
            ]),
        )
        .fill_width(),
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

/// Light/dark appearance picker. The group owns the `theme_mode` binding
/// directly; the wrapper theme provider re-resolves on every change.
fn settings_theme_row(model: Rc<AppModel>) -> AnyView {
    let options = vec![
        RadioOption::new(ThemeMode::System, ThemeMode::System.title()),
        RadioOption::new(ThemeMode::Light, ThemeMode::Light.title()),
        RadioOption::new(ThemeMode::Dark, ThemeMode::Dark.title()),
    ];
    let picker = AnyView::new(
        RadioGroup::try_new(model.theme_mode.binding(), options)
            .expect("system, light and dark are distinct theme modes")
            .horizontal(true),
    );
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
                            Text::new("主题")
                                .semantic_font(SemanticFontRef::body())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Text::new("跟随系统深浅色，或手动指定浅色 / 暗色")
                                .line_limit(Some(2))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        )),
                    )
                    .frame_min_width(0.0)
                    .flex(1.0),
                ),
                picker,
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
