//! One window, three bands: a folder index down the left edge, a title row that
//! carries the archive's identity and every action, and the file list. There is
//! no footer — where the contents land is stated right next to the button that
//! starts the work.

use std::cmp::Ordering;
use std::path::PathBuf;
use std::rc::Rc;

use cove_ui::compose::adaptive::SurfaceEnvironment;
use cove_ui::compose::modifiers::layout::EdgeInsets;
use cove_ui::compose::modifiers::style::AppearanceExt;
use cove_ui::compose::prelude::*;
use cove_ui::compose::primitives::data::table::{ColumnVisibility, ComparisonResult};
use cove_ui::compose::primitives::feedback::ProgressView;
use cove_ui::compose::primitives::layout::{
    ScrollView, SplitAxis, SplitConstraints, SplitView,
};
use cove_ui::compose::primitives::media::ui_icon_catalog::{action, data, file, media, navigation};
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
use crate::model::{file_name, AppModel, ExtractRecord, LoadedArchive, Phase, Tone, ARCHIVE_EXTENSIONS};
use crate::theme;
use mugenzip_core::format_size;

const TOOLBAR: f32 = 56.0;
const CRUMB_BAR: f32 = 34.0;
const PROGRESS_BAR: f32 = 34.0;
const GUTTER: f32 = 20.0;

pub struct RootView {
    pub model: Rc<AppModel>,
}

impl View for RootView {
    fn body(&self, cx: &Scope) -> AnyView {
        self.model.ensure_platform_hooks();
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
        let shell_model = model.clone();
        AnyView::new(
            FileDropZone::new(
                move |files| drop.open_dropped(files),
                workspace(shell_model, safe_top),
            )
                .accepted_extensions(ARCHIVE_EXTENSIONS)
                .maximum_files(1)
                .shows_chrome(false)
                .is_targeted(model.drop_targeted.binding())
                .on_targeted(move |is_targeted| targeted.drop_targeted.set(is_targeted))
                .fill()
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

/// The window never changes its frame: the index stays on the left, the title
/// row stays on top, only the middle swaps between the archive and the
/// "open something" invitation.
fn workspace(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    // Same shell as the Gallery: the sidebar is not painted at all, so the
    // window's own material is what you see behind it; the content sits on one
    // floating card. The split's divider lane is invisible — the card's edge
    // does the separating — but it still drags the fraction.
    // Padding has to live on a container node: put it on the card itself and
    // the fill swallows its own margin, leaving the card flush to the edge.
    let card = AnyView::new(
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                0.0,
                AnyView::new(views![AnyView::new(content(model.clone(), safe_top))
                    .semantic_background(SemanticColorRef::background())
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
            minimum_second: 520.0,
            ..SplitConstraints::new(0.12, 0.34)
        })
        .fill(),
    )
}

/// The sidebar is not a second file list. It only switches context roots —
/// where the user is working — while descending into a folder stays the
/// content area's job (double-click, breadcrumb, back chevron). That is the
/// whole reason it exists, so it never lists the archive's own folders.
fn sidebar(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    let archive = model.archive.get();
    let mut rows: Vec<AnyView> = Vec::new();

    if let Some(current) = &archive {
        rows.push(section_label("正在处理", 0.0));
        rows.push(job_card(current));
    }

    // 落点：压缩包落地的目录，不是可浏览的树。这里只换根，不下钻。
    if archive.is_some() {
        rows.push(section_label("解压到", 16.0));
        for (label, path) in model.destination_choices() {
            let pick = model.clone();
            let selected = model.destination_is(&path);
            let target = path.clone();
            rows.push(nav_row(
                file::folder_open(),
                label,
                0,
                selected,
                move || pick.set_destination(target.clone()),
            ));
        }
        let choose = model.clone();
        rows.push(sub_row("选择其他位置…", move || choose.choose_destination()));
    }

    // 队列：排在当前包后面的活，以及正在跑的那一件。
    let busy = model.phase.get() == Phase::Extracting;
    let queued = model.queued();
    if let Some(current) = &archive
        && (busy || !queued.is_empty())
    {
        rows.push(section_label("队列", 16.0));
        if busy {
            rows.push(active_row(model.clone(), current));
        }
        for path in queued {
            let open = model.clone();
            rows.push(nav_row(
                file::package(),
                file_name(&path),
                0,
                false,
                move || open.open_recent(path.clone()),
            ));
        }
    }

    let history = model.history.get();
    if !history.is_empty() {
        rows.push(section_label("最近解压", 16.0));
        for record in history.into_iter().take(3) {
            rows.push(history_row(record));
        }
    }

    // The sections can outgrow the pane — a long queue or a deep history — so
    // they scroll inside it instead of being clipped by a fixed box.
    let list = AnyView::new(
        ScrollView::vertical(
            AnyView::new(vstack(0.0, rows))
                .padding_insets(EdgeInsets::new(safe_top + 16.0, 8.0, 12.0, 8.0))
                .fill_width(),
        )
        .combine_accessibility(false),
    )
    .fill_width()
    .fill_height();

    AnyView::new(
        Column::new(HorizontalAlignment::Leading, 0.0, AnyView::new(list))
            .fill_width()
            .fill_height(),
    )
}

fn job_card(archive: &LoadedArchive) -> AnyView {
    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            9.0,
            AnyView::new(views!(
                AnyView::new(
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(file::archive()),
                        Some(15.0),
                        None,
                    ))
                    .semantic_foreground(SemanticColorRef::on_accent()),
                )
                .frame_size(30.0, 30.0)
                .corner_radius(7.0)
                .semantic_background(SemanticColorRef::accent()),
                AnyView::new(
                    Column::new(
                        HorizontalAlignment::Leading,
                        2.0,
                        AnyView::new(views!(
                            Text::new(archive.name.clone())
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::body_strong())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Text::new(archive.meta_line())
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        )),
                    )
                    .frame_min_width(0.0),
                ),
            )),
        )
        .padding_insets(EdgeInsets::new(9.0, 9.0, 10.0, 9.0))
        .fill_width()
        .corner_radius(9.0)
        .semantic_background(SemanticColorRef::surface()),
    )
}

/// "解压中 · 38%" plus the one control that matters while it runs.
fn active_row(model: Rc<AppModel>, archive: &LoadedArchive) -> AnyView {
    let fraction = model.progress.get().fraction.clamp(0.0, 1.0);
    let percent = (fraction * 100.0).round();
    let cancel = model.clone();
    let header = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            6.0,
            AnyView::new(views!(
                Text::new(archive.name.clone())
                    .line_limit(Some(1))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                Spacer::new(0.0),
                AnyView::new(
                    Button::new(
                        AnyView::new(
                            Text::new("取消")
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        ),
                        move || cancel.cancel_extract(),
                    )
                    .accessibility_text("取消解压")
                    .button_style(GhostButtonStyle)
                    .control_size(ControlSize::Small),
                ),
            )),
        )
        .fill_width(),
    );

    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            5.0,
            AnyView::new(views!(
                header,
                AnyView::new(
                    Text::new(format!(
                        "解压中 {percent:.0}% · {}",
                        model.destination_name()
                    ))
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
                ),
                AnyView::new(
                    AnyView::new(
                        ProgressView::new(Some(fraction as f64))
                            .height(3.0),
                    )
                    .fill_width(),
                ),
            )),
        )
        .padding_insets(EdgeInsets::new(8.0, 9.0, 9.0, 9.0))
        .fill_width()
        .corner_radius(8.0)
        .semantic_background(SemanticColorRef::surface()),
    )
}

fn history_row(record: ExtractRecord) -> AnyView {
    AnyView::new(
        Row::new(
            VerticalAlignment::Top,
            8.0,
            AnyView::new(views!(
                AnyView::new(EmptyView)
                    .frame_size(6.0, 6.0)
                    .corner_radius(3.0)
                    .semantic_background(SemanticColorRef::border_strong()),
                AnyView::new(
                    Column::new(
                        HorizontalAlignment::Leading,
                        1.0,
                        AnyView::new(views!(
                            Text::new(record.name)
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::body())
                                .semantic_foreground(SemanticColorRef::on_surface()),
                            Text::new(format!("→ {} · {}", record.destination, record.at))
                                .line_limit(Some(1))
                                .truncating_tail()
                                .semantic_font(SemanticFontRef::caption())
                                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        )),
                    )
                    .frame_min_width(0.0),
                ),
            )),
        )
        .padding_insets(EdgeInsets::new(6.0, 9.0, 6.0, 9.0))
        .fill_width(),
    )
}

/// A quiet text-only action, used for "选择其他位置…".
fn sub_row(label: &str, action: impl Fn() + 'static) -> AnyView {
    AnyView::new(
        AnyView::new(
            Button::new(
                Row::new(
                    VerticalAlignment::Center,
                    0.0,
                    AnyView::new(views!(
                        Text::new(label.to_owned())
                            .line_limit(Some(1))
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        Spacer::new(0.0),
                    )),
                )
                .fill_width(),
                action,
            )
            .button_style(GhostButtonStyle)
            .control_size(ControlSize::Small)
            .fill_width(),
        )
        .padding_insets(EdgeInsets::new(1.0, 24.0, 1.0, 8.0))
        .fill_width(),
    )
}

fn content(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    let busy = model.phase.get() == Phase::Extracting;
    let has_archive = model.archive.get().is_some();

    let middle: AnyView = if has_archive {
        let progress: AnyView = if busy {
            progress_strip(model.clone())
        } else {
            AnyView::new(EmptyView)
        };
        AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                0.0,
                AnyView::new(views!(
                    progress,
                    crumb_bar(model.clone()),
                    list_area(model.clone())
                )),
            )
            .flex(1.0)
            .fill_width(),
        )
    } else if model.phase.get() == Phase::Loading {
        reading()
    } else {
        invitation(model.clone())
    };

    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(toolbar(model, safe_top), middle,)),
        )
        .flex(1.0)
        .fill_height()
        .semantic_background(SemanticColorRef::surface_sunken()),
    )
}

// ---------------------------------------------------------------------------
// toolbar
// ---------------------------------------------------------------------------

fn toolbar(model: Rc<AppModel>, safe_top: f32) -> AnyView {
    let archive = model.archive.get();
    let loaded = archive.is_some();
    let at_root = model.directory.get().is_empty();

    // Reference language: a back chevron, the archive's name, then the tools
    // the window offers — glyph over a small label, no bar behind them.
    let back = model.clone();
    let back_button = if loaded { AnyView::new(
        Button::new(
            AnyView::new(ButtonIcon::new(
                ButtonIconSource::resource(navigation::chevron_left()),
                Some(15.0),
                None,
            ))
            .semantic_foreground(SemanticColorRef::on_surface()),
            move || back.navigate_up(),
        )
        .enabled(loaded && !at_root && !model.is_busy())
        .accessibility_text("上一级")
        .tooltip("上一级")
        .button_style(PlainButtonStyle)
        .control_size(ControlSize::Small),
    ) } else {
        AnyView::new(AnyView::new(EmptyView).frame_width(24.0))
    };

    let title_block: AnyView = match &archive {
        Some(archive) => AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                2.0,
                AnyView::new(views!(
                    Text::new(archive.name.clone())
                        .line_limit(Some(1))
                        .truncating_tail()
                        .semantic_font(SemanticFontRef::title())
                        .semantic_foreground(SemanticColorRef::on_surface()),
                    Text::new(identity_subtitle(&model, archive))
                        .line_limit(Some(1))
                        .truncating_tail()
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                )),
            )
            .frame_min_width(0.0),
        ),
        None => AnyView::new(
            Column::new(
                HorizontalAlignment::Leading,
                2.0,
                AnyView::new(views!(
                    Text::new("MugenZip")
                        .semantic_font(SemanticFontRef::title())
                        .semantic_foreground(SemanticColorRef::on_surface()),
                    Text::new("简单、快速的压缩包工具")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                )),
            ),
        ),
    };

    let open = model.clone();
    let open_action = toolbar_action(file::archive(), "打开", true, false, move || open.open_dialog());

    let run = model.clone();
    let extract = toolbar_action(
        action::download(),
        "解压",
        loaded && !model.is_busy(),
        true,
        move || run.extract(),
    );

    let search: AnyView = if loaded {
        AnyView::new(
            SearchField::new(model.search.binding())
                .placeholder("搜索")
                .frame_width(120.0)
                .control_size(ControlSize::Small),
        )
    } else {
        AnyView::new(EmptyView)
    };

    let tools = AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            0.0,
            AnyView::new(views!(
                search,
                open_action,
                extract,
                more_pill(model.clone()),
            )),
        ),
    );

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views!(
                back_button,
                title_block,
                Spacer::new(0.0),
                tools,
            )),
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
    if model.phase.get() == Phase::Extracting {
        return format!("正在解压 · {}", model.progress.get().label);
    }
    if status.tone == Tone::Error {
        return if status.detail.trim().is_empty() {
            status.text
        } else {
            format!("{} · {}", status.text, status.detail)
        };
    }
    // The footer used to state this; now the title carries it.
    format!("{} · 解压到 {}", archive.meta_line(), model.destination_name())
}

/// Toolbar language from the reference: glyph over a small label, no chrome
/// until the pointer arrives. The primary action is the only accent-tinted one.
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

/// The reference's secondary control: a hairline pill that reads "更多 ⌄".
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

/// The overflow command list. Everything the footer used to carry lives here
/// or in the title line.
fn menu_entries(model: Rc<AppModel>) -> Vec<MenuEntry> {
    let wrap_on = model.wrap_in_folder.get();
    let toggle = model.clone();
    let choose = model.clone();
    let all = model.clone();
    let copy = model.clone();
    let open = model.clone();
    vec![
        MenuEntry::Item(MenuItem::new(1, "更改解压位置…", move || {
            choose.choose_destination()
        })),
        MenuEntry::Item(
            MenuItem::new(2, "解压到同名文件夹", move || toggle.toggle_wrap()).selected(wrap_on),
        ),
        MenuEntry::separator(3),
        MenuEntry::Item(MenuItem::new(4, "打开压缩包…", move || open.open_dialog())),
        MenuEntry::Item(MenuItem::new(5, "全选", move || all.select_all()).shortcut("⌘A")),
        MenuEntry::Item(MenuItem::new(6, "复制档案路径", move || copy.copy_path(String::new()))),
    ]
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

    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            4.0,
            AnyView::new(views!(
                Row::new(
                    VerticalAlignment::Center,
                    8.0,
                    AnyView::new(views!(
                        Text::new(current)
                            .line_limit(Some(1))
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface())
                            .frame_min_width(0.0)
                            .flex(1.0),
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
                    .height(4.0)
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

    let selected = model.selection.get().len();
    let summary = if selected > 0 {
        let (count, bytes) = model.selection_scope();
        format!("已选 {selected} 项 · {} · {count} 条目", format_size(bytes))
    } else {
        let items = model.current_items();
        let folders = items.iter().filter(|item| item.folder).count();
        let files = items.len() - folders;
        format!("{folders} 个文件夹 · {files} 个文件")
    };

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            8.0,
            AnyView::new(views!(
                AnyView::new(breadcrumb).frame_min_width(0.0).flex(1.0),
                Text::new(summary)
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )),
        )
        .padding_insets(EdgeInsets::new(4.0, GUTTER, 4.0, GUTTER))
        .frame(None, None, None, Some(CRUMB_BAR), None, None)
        .fill_width(),
    )
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
        row_height: 38.0,
        header_height: 32.0,
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
                Text::new(if row.folder { String::new() } else { row.modified.clone() })
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
// invitation / loading
// ---------------------------------------------------------------------------

/// The reference's hero: one mark, one sentence, two cards. A first-run window
/// should say what the app is for, not show an empty table.
fn invitation(model: Rc<AppModel>) -> AnyView {
    let open = model.clone();
    let open_card = action_card(
        action::plus(),
        "打开压缩包",
        "浏览、选择并解压一个压缩包",
        move || open.open_dialog(),
    );

    let recent_path: Option<PathBuf> = model.recent.get().into_iter().next();
    let recent_model = model.clone();
    let recent_card = match recent_path {
        Some(path) => action_card(data::clock(), "最近打开", file_name(&path), move || {
            recent_model.open_recent(path.clone())
        }),
        None => action_card(data::clock(), "最近打开", "还没有打开过压缩包", || {}),
    };

    AnyView::new(
        Column::new(
            HorizontalAlignment::Center,
            0.0,
            AnyView::new(views!(
                Spacer::new(0.0),
                tile(
                    file::package(),
                    88.0,
                    SemanticColorRef::accent_muted(),
                    SemanticColorRef::accent()
                ),
                Text::new("MugenZip")
                    .alignment(TextAlignment::Center)
                    .semantic_font(SemanticFontRef::display())
                    .semantic_foreground(SemanticColorRef::on_surface())
                    .padding_insets(EdgeInsets::new(24.0, 0.0, 0.0, 0.0)),
                Text::new("简单、快速的压缩包工具")
                    .alignment(TextAlignment::Center)
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface_variant())
                    .padding_insets(EdgeInsets::new(8.0, 0.0, 0.0, 0.0)),
                Row::new(
                    VerticalAlignment::Center,
                    16.0,
                    AnyView::new(views!(open_card, recent_card)),
                )
                .padding_insets(EdgeInsets::new(30.0, 0.0, 0.0, 0.0)),
                Text::new("也可以把压缩包拖到窗口任意位置")
                    .alignment(TextAlignment::Center)
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted())
                    .padding_insets(EdgeInsets::new(18.0, 0.0, 0.0, 0.0)),
                Spacer::new(0.0),
            )),
        )
        .fill_width()
        .flex(1.0),
    )
}

/// A generous white card: glyph badge, one bold line, one quiet line.
fn action_card(
    glyph: cove_ui::imaging::BundleImageResource,
    title: &str,
    subtitle: impl Into<String>,
    action: impl Fn() + 'static,
) -> AnyView {
    let card = Button::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                tile(
                    glyph,
                    30.0,
                    SemanticColorRef::accent_muted(),
                    SemanticColorRef::accent()
                ),
                Text::new(title.to_owned())
                    .semantic_font(SemanticFontRef::body_strong())
                    .semantic_foreground(SemanticColorRef::on_surface())
                    .padding_insets(EdgeInsets::new(14.0, 0.0, 0.0, 0.0)),
                Text::new(subtitle.into())
                    .line_limit(Some(2))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted())
                    .padding_insets(EdgeInsets::new(4.0, 0.0, 0.0, 0.0)),
            )),
        )
        .frame_width(286.0)
        .frame_height(152.0)
        .padding_insets(EdgeInsets::new(18.0, 18.0, 18.0, 18.0))
        .semantic_background(SemanticColorRef::surface())
        .corner_radius(12.0)
        .semantic_border(SemanticColorRef::border(), 1.0)
        .shadow(Color::from_rgba8(0, 0, 0, 12), 8.0, 0.0, 2.0),
        action,
    )
    .accessibility_text(title)
    .button_style(PlainButtonStyle);

    AnyView::new(card)
}

fn reading() -> AnyView {
    AnyView::new(
        Column::new(
            HorizontalAlignment::Center,
            0.0,
            AnyView::new(views!(
                Spacer::new(0.0),
                Text::new("正在打开压缩包…")
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface_variant()),
                AnyView::new(
                    ProgressView::new(None)
                        .height(3.0)
                        .tone(StatusTone::Info)
                        .frame_width(180.0)
                )
                .padding_insets(EdgeInsets::new(14.0, 0.0, 0.0, 0.0)),
                Spacer::new(0.0),
            )),
        )
        .fill_width()
        .flex(1.0),
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
            Text::new(title)
                .line_limit(Some(1))
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        )
        .fill_width()
        .padding_insets(EdgeInsets::new(top, 22.0, 8.0, 0.0)),
    )
}

fn hint_row(text: &str) -> AnyView {
    AnyView::new(
        AnyView::new(
            Text::new(text.to_owned())
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        )
        .fill_width()
        .padding_insets(EdgeInsets::new(2.0, 22.0, 8.0, 0.0)),
    )
}

fn nav_row(
    glyph: cove_ui::imaging::BundleImageResource,
    label: String,
    depth: usize,
    selected: bool,
    action: impl Fn() + 'static,
) -> AnyView {
    let text_color = if selected {
        SemanticColorRef::accent()
    } else {
        SemanticColorRef::on_surface()
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

    let button = Button::new(
        Row::new(
            VerticalAlignment::Center,
            9.0,
            AnyView::new(views!(
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
            )),
        )
        .padding_insets(EdgeInsets::new(0.0, (depth.min(3) as f32) * 14.0, 0.0, 0.0))
        .fill_width(),
        action,
    )
    .button_style(GhostButtonStyle)
    .control_size(ControlSize::Small)
    .fill_width();

    let body = AnyView::new(
        AnyView::new(button)
            .frame(None, None, None, Some(32.0), None, None)
            .padding_insets(EdgeInsets::new(0.0, 0.0, 0.0, 0.0)),
    );
    let row = if selected {
        AnyView::new(
            AnyView::new(body)
                .corner_radius(6.0)
                .semantic_background(SemanticColorRef::accent_muted()),
        )
    } else {
        AnyView::new(body)
    };

    AnyView::new(
        AnyView::new(row.fill_width())
            .padding_insets(EdgeInsets::new(1.0, 8.0, 1.0, 8.0))
            .fill_width(),
    )
}

fn tile(
    glyph: cove_ui::imaging::BundleImageResource,
    size: f32,
    background: SemanticColorRef,
    tint: SemanticColorRef,
) -> AnyView {
    AnyView::new(
        AnyView::new(
            AnyView::new(ButtonIcon::new(
                ButtonIconSource::resource(glyph),
                Some(size * 0.5),
                None,
            ))
            .semantic_foreground(tint),
        )
        .frame_width(size)
        .frame_height(size)
        .corner_radius(size * 0.28)
        .semantic_background(background),
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
