//! A Mac-native archive workspace: an outline sidebar for structure, one
//! file list for content, and a single footer that states where the contents
//! are about to land.

use std::cmp::Ordering;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use cove_ui::compose::modifiers::layout::EdgeInsets;
use cove_ui::compose::modifiers::style::AppearanceExt;
use cove_ui::compose::prelude::*;
use cove_ui::compose::primitives::data::lists::tree::TreeOptions;
use cove_ui::compose::primitives::data::table::{ColumnVisibility, ComparisonResult};
use cove_ui::compose::primitives::media::ui_icon_catalog::{
    action, data, file, media, navigation, status,
};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, GhostButtonStyle, PlainButtonStyle, PrimaryButtonStyle,
};
use cove_ui::compose::theme::ThemeViewExt;
use cove_ui::composition::{
    DynamicField, EmptyView, HorizontalAlignment, Spacer, VerticalAlignment, views,
};
use cove_ui::core::Color;

use super::browser::FileItem;
use super::model::{AppModel, ARCHIVE_EXTENSIONS, LoadedArchive, Phase, Tone, file_name};
use super::theme;
use crate::engine::format_size;

const TOP_BAR: f32 = 54.0;
const SIDEBAR_WIDTH: f32 = 208.0;
const PATH_BAR: f32 = 40.0;
const FOOTER: f32 = 64.0;
const GUTTER: f32 = 16.0;

pub struct RootView {
    pub model: Rc<AppModel>,
}

impl View for RootView {
    fn body(&self, _cx: &Scope) -> AnyView {
        self.model.ensure_platform_hooks();
        let model = self.model.clone();

        let main: AnyView = match (model.archive.get(), model.phase.get()) {
            (None, Phase::Loading) => reading(),
            (None, _) => idle(model.clone()),
            (Some(archive), _) => workspace(model.clone(), archive),
        };

        let footer: AnyView = if model.archive.get().is_some() {
            action_bar(model.clone())
        } else {
            AnyView::new(EmptyView)
        };

        let shell = Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                top_bar(model.clone()),
                Divider::horizontal(),
                main.flex(1.0),
                footer,
            )),
        )
        .fill()
        .semantic_background(SemanticColorRef::background())
        .theme(theme::paper());

        let drop = model.clone();
        let targeted = model.clone();
        AnyView::new(
            FileDropZone::new(move |files| drop.open_dropped(files), shell)
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
// top bar
// ---------------------------------------------------------------------------

fn top_bar(model: Rc<AppModel>) -> AnyView {
    let archive = model.archive.get();
    let loaded = archive.is_some();

    let identity: AnyView = match &archive {
        Some(archive) => AnyView::new(Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views!(
                tile(file::package(), 30.0, SemanticColorRef::accent_muted(), SemanticColorRef::accent()),
                Column::new(
                    HorizontalAlignment::Leading,
                    1.0,
                    AnyView::new(views!(
                        Text::new(archive.name.clone())
                            .line_limit(Some(1))
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::headline())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        Text::new(archive.meta_line())
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    )),
                )
                .frame_min_width(0.0),
            )),
        )
        .frame_min_width(0.0)),
        None => AnyView::new(Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views!(
                tile(file::package(), 30.0, SemanticColorRef::accent_muted(), SemanticColorRef::accent()),
                Text::new("MugenZip")
                    .semantic_font(SemanticFontRef::headline())
                    .semantic_foreground(SemanticColorRef::on_surface()),
            )),
        )),
    };

    let open = model.clone();
    let open_action = toolbar_action(file::archive(), "打开", true, move || open.open_dialog());

    let extract = model.clone();
    let extract_action = toolbar_action(action::download(), "解压", loaded && !model.is_busy(), move || {
        extract.extract()
    });

    let up = model.clone();
    let up_action = toolbar_action(
        navigation::arrow_up(),
        "上级",
        loaded && !model.directory.get().is_empty() && !model.is_busy(),
        move || up.navigate_up(),
    );

    let more = more_menu(model.clone());

    let search: AnyView = if loaded {
        AnyView::new(
            SearchField::new(model.search.binding())
                .placeholder("搜索")
                .frame_width(170.0)
                .control_size(ControlSize::Small),
        )
    } else {
        AnyView::new(EmptyView)
    };

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views!(
                identity,
                Spacer::new(0.0),
                open_action,
                extract_action,
                up_action,
                ToolbarSeparator,
                search,
                more,
            )),
        )
        .padding_insets(EdgeInsets::new(6.0, GUTTER, 6.0, GUTTER))
        .frame_height(TOP_BAR)
        .fill_width()
        .semantic_background(SemanticColorRef::background()),
    )
}

/// The mockup's toolbar language: glyph over a small label, no chrome until
/// the pointer arrives.
fn toolbar_action(
    glyph: cove_ui::imaging::BundleImageResource,
    title: &'static str,
    enabled: bool,
    action: impl Fn() + 'static,
) -> AnyView {
    AnyView::new(
        Button::new(
            Column::new(
                HorizontalAlignment::Center,
                2.0,
                AnyView::new(views!(
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(glyph),
                        Some(17.0),
                        None,
                    ))
                    .semantic_foreground(SemanticColorRef::on_surface_variant()),
                    Text::new(title)
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                )),
            ),
            action,
        )
        .enabled(enabled)
        .accessibility_text(title)
        .tooltip(title)
        .button_style(GhostButtonStyle)
        .control_size(ControlSize::Large),
    )
}

fn more_menu(model: Rc<AppModel>) -> AnyView {
    let dialog = model.clone();
    let named = model.clone();
    let copy = model.clone();
    let entries = vec![
        MenuEntry::Item(MenuItem::new(
            1,
            "解压到…",
            move || dialog.choose_destination(),
        )),
        MenuEntry::Item(MenuItem::new(2, "打开压缩包…", move || named.open_dialog())),
        MenuEntry::separator(3),
        MenuEntry::Item(MenuItem::new(4, "复制当前路径", move || {
            copy.copy_path(copy.directory.get())
        })),
    ];
    AnyView::new(
        MenuButton::new(
            entries,
            AnyView::new(
                ButtonIcon::new(ButtonIconSource::resource(action::more_horizontal()), Some(17.0), None)
                    .semantic_foreground(SemanticColorRef::on_surface_variant()),
            ),
        )
        .ghost()
        .accessibility_label("更多操作")
        .tooltip("更多操作"),
    )
}

// ---------------------------------------------------------------------------
// workspace
// ---------------------------------------------------------------------------

fn workspace(model: Rc<AppModel>, archive: Arc<LoadedArchive>) -> AnyView {
    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            0.0,
            AnyView::new(views!(
                sidebar(model.clone(), archive),
                Divider::vertical(),
                content(model.clone()),
            )),
        )
        .fill(),
    )
}

fn sidebar(model: Rc<AppModel>, archive: Arc<LoadedArchive>) -> AnyView {
    let roots = vec![archive.browser.archive_root_item()];
    let children_archive = archive.clone();
    let nav = model.clone();

    let mut options = TreeOptions::<FileItem, String>::default();
    // No selection band in the outline: the current folder is marked by ink,
    // the way Finder's sidebar does it.
    let _ = &options.selection;
    options.expansion.default_expand_all = true;
    options.expansion.auto_expand_parent = true;
    options.layout.row_height = 28.0;
    options.layout.indentation = 14.0;
    options.layout.leading_padding = 10.0;
    options.slots.icon = Some(Rc::new(|item: &FileItem| {
        let (glyph, tint) = if item.id.is_empty() {
            (file::archive(), SemanticColorRef::accent())
        } else {
            (file::folder(), SemanticColorRef::on_surface_variant())
        };
        AnyView::new(
            AnyView::new(ButtonIcon::new(
                ButtonIconSource::resource(glyph),
                Some(14.0),
                None,
            ))
            .semantic_foreground(tint),
        )
    }));
    options.events.on_select = Some(Rc::new(move |item: FileItem| nav.enter_directory(item.id)));

    let tree = Tree::new(
        roots,
        |item: &FileItem| item.id.clone(),
        move |item: &FileItem| children_archive.browser.subfolders(&item.id),
        {
            let current = model.clone();
            move |item: &FileItem, _state| {
                let is_here = current.directory.get() == item.id;
                AnyView::new(
                    Text::new(item.name.clone())
                        .truncating_tail()
                        .semantic_font(if is_here {
                            SemanticFontRef::body_strong()
                        } else {
                            SemanticFontRef::body()
                        })
                        .semantic_foreground(if is_here {
                            SemanticColorRef::accent()
                        } else {
                            SemanticColorRef::on_surface()
                        }),
                )
            }
        },
    )
    .options(options);

    let stats = Column::new(
        HorizontalAlignment::Leading,
        2.0,
        AnyView::new(views!(
            Text::new(format!("{} 个项目", archive.entry_count))
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_variant()),
            Text::new(format_size(archive.total_bytes))
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        )),
    )
    .padding_insets(EdgeInsets::new(12.0, 16.0, 12.0, 16.0))
    .fill_width();

    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                Text::new("目录")
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted())
                    .padding_insets(EdgeInsets::new(14.0, 16.0, 4.0, 16.0)),
                AnyView::new(tree.flex(1.0)),
                Divider::horizontal(),
                stats,
            )),
        )
        .frame_width(SIDEBAR_WIDTH)
        .fill_height()
        .semantic_background(SemanticColorRef::surface_variant()),
    )
}

fn content(model: Rc<AppModel>) -> AnyView {
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                path_bar(model.clone()),
                list_area(model.clone()),
            )),
        )
        .flex(1.0)
        .fill_height(),
    )
}

fn path_bar(model: Rc<AppModel>) -> AnyView {
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
    let breadcrumb: AnyView = Breadcrumb::try_new(crumbs, 5, move |path| select.enter_directory(path))
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
        .frame_height(PATH_BAR)
        .fill_width(),
    )
}

fn list_area(model: Rc<AppModel>) -> AnyView {
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(file_table(model)),
        )
        .padding_insets(EdgeInsets::new(0.0, GUTTER, GUTTER, GUTTER))
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
        header_height: 34.0,
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
// footer
// ---------------------------------------------------------------------------

fn action_bar(model: Rc<AppModel>) -> AnyView {
    let body: AnyView = if model.phase.get() == Phase::Extracting {
        extracting(model.clone())
    } else {
        ready_bar(model.clone())
    };
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                Divider::horizontal(),
                AnyView::new(body).frame_height(FOOTER).fill_width(),
            )),
        )
        .fill_width()
        .semantic_background(SemanticColorRef::surface()),
    )
}

fn ready_bar(model: Rc<AppModel>) -> AnyView {
    let choose = model.clone();
    let destination = Button::new(
        Row::new(
            VerticalAlignment::Center,
            7.0,
            AnyView::new(views!(
                AnyView::new(ButtonIcon::new(
                    ButtonIconSource::resource(file::folder_open()),
                    Some(13.0),
                    None,
                ))
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
                Text::new(model.destination_label())
                    .line_limit(Some(1))
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::mono())
                    .semantic_foreground(SemanticColorRef::on_surface()),
                AnyView::new(ButtonIcon::new(
                    ButtonIconSource::resource(navigation::chevron_down()),
                    Some(11.0),
                    None,
                ))
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )),
        ),
        move || choose.choose_destination(),
    )
    .accessibility_text("更改解压位置")
    .tooltip("选择解压目标位置")
    .button_style(GhostButtonStyle)
    .control_size(ControlSize::Small);

    let status = model.status.get();
    let tone_color = match status.tone {
        Tone::Ok => SemanticColorRef::on_surface_muted(),
        Tone::Busy => SemanticColorRef::accent(),
        Tone::Error => SemanticColorRef::error(),
    };
    let glyph = match status.tone {
        Tone::Ok => status::check_circle(),
        Tone::Busy => status::loader(),
        Tone::Error => status::alert_circle(),
    };
    let detail = if status.detail.trim().is_empty() {
        status.text.clone()
    } else {
        format!("{} · {}", status.text, status.detail)
    };

    let left: AnyView = AnyView::new(Column::new(
        HorizontalAlignment::Leading,
        4.0,
        AnyView::new(views!(
            Row::new(
                VerticalAlignment::Center,
                6.0,
                AnyView::new(views!(
                    Text::new("解压到")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    destination,
                )),
            )
            .frame_min_width(0.0),
            Row::new(
                VerticalAlignment::Center,
                5.0,
                AnyView::new(views!(
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(glyph),
                        Some(11.0),
                        None,
                    ))
                    .semantic_foreground(tone_color.clone()),
                    Text::new(detail)
                        .line_limit(Some(1))
                        .truncating_tail()
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(tone_color),
                )),
            )
            .frame_min_width(0.0),
        )),
    )
    .frame_min_width(0.0)
    .flex(1.0));

    let wrap_box = Checkbox::labeled_from_boolean("同名文件夹", model.wrap_in_folder.binding())
        .control_size(ControlSize::Small);

    let run = model.clone();
    let primary = Button::new(
        Row::new(
            VerticalAlignment::Center,
            6.0,
            AnyView::new(views!(
                ButtonIcon::new(ButtonIconSource::resource(action::download()), Some(14.0), None),
                Text::new(model.action_label()),
            )),
        ),
        move || run.extract(),
    )
    .enabled(!model.is_busy())
    .accessibility_text("解压")
    .tooltip("解压到左侧显示的位置")
    .button_style(PrimaryButtonStyle)
    .control_size(ControlSize::Small);

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            16.0,
            AnyView::new(views!(left, wrap_box, primary)),
        )
        .padding_insets(EdgeInsets::new(12.0, GUTTER, 12.0, GUTTER))
        .fill_width(),
    )
}

fn extracting(model: Rc<AppModel>) -> AnyView {
    let progress = model.progress.get();
    let cancel = model.clone();
    let current = if progress.current_file.trim().is_empty() {
        progress.label.clone()
    } else {
        progress.current_file.clone()
    };

    let left: AnyView = AnyView::new(Column::new(
        HorizontalAlignment::Leading,
        7.0,
        AnyView::new(views!(
            Row::new(
                VerticalAlignment::Center,
                8.0,
                AnyView::new(views!(
                    Text::new(current)
                        .line_limit(Some(1))
                        .truncating_tail()
                        .semantic_font(SemanticFontRef::body())
                        .semantic_foreground(SemanticColorRef::on_surface())
                        .frame_min_width(0.0)
                        .flex(1.0),
                    Text::new(progress.label.clone())
                        .semantic_font(SemanticFontRef::mono())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                )),
            ),
            ProgressView::new(Some(progress.fraction))
                .height(4.0)
                .tone(StatusTone::Info)
                .fill_width(),
        )),
    )
    .frame_min_width(0.0)
    .flex(1.0));

    let cancel_button = Button::new(
        Text::new("取消")
            .semantic_font(SemanticFontRef::body())
            .semantic_foreground(SemanticColorRef::on_surface()),
        move || cancel.cancel_extract(),
    )
    .accessibility_text("取消解压")
    .button_style(GhostButtonStyle)
    .control_size(ControlSize::Small);

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            16.0,
            AnyView::new(views!(left, cancel_button)),
        )
        .padding_insets(EdgeInsets::new(12.0, GUTTER, 12.0, GUTTER))
        .fill_width(),
    )
}

// ---------------------------------------------------------------------------
// empty / loading
// ---------------------------------------------------------------------------

fn idle(model: Rc<AppModel>) -> AnyView {
    let open = model.clone();
    let open_card = action_card(
        action::plus(),
        true,
        "打开压缩包",
        "浏览、选择并解压一个压缩包",
        move || open.open_dialog(),
    );

    let recent_path: Option<PathBuf> = model.recent.get().into_iter().next();
    let recent_model = model.clone();
    let recent_card = match recent_path.clone() {
        Some(path) => action_card(
            data::clock(),
            false,
            "最近打开",
            file_name(&path),
            move || recent_model.open_recent(path.clone()),
        ),
        None => action_card(
            data::clock(),
            false,
            "最近打开",
            "还没有打开过压缩包",
            || {},
        ),
    };

    // Rhythm comes from padding, not from spacer children: a fixed frame on an
    // empty view collapses inside a flex column.
    let caption = Column::new(
        HorizontalAlignment::Center,
        0.0,
        AnyView::new(views!(
            Text::new("MugenZip")
                .alignment(TextAlignment::Center)
                .semantic_font(SemanticFontRef::title())
                .semantic_foreground(SemanticColorRef::on_surface())
                .padding_insets(EdgeInsets::new(24.0, 0.0, 0.0, 0.0)),
            Text::new("简单、快速的压缩包工具")
                .alignment(TextAlignment::Center)
                .semantic_font(SemanticFontRef::body())
                .semantic_foreground(SemanticColorRef::on_surface_variant())
                .padding_insets(EdgeInsets::new(8.0, 0.0, 0.0, 0.0)),
        )),
    );

    let cards = Row::new(
        VerticalAlignment::Center,
        16.0,
        AnyView::new(views!(open_card, recent_card)),
    );

    AnyView::new(
        Column::new(
            HorizontalAlignment::Center,
            0.0,
            AnyView::new(views!(
                Spacer::new(0.0),
                tile(file::package(), 76.0, SemanticColorRef::accent_muted(), SemanticColorRef::accent()),
                caption,
                AnyView::new(cards).padding_insets(EdgeInsets::new(30.0, 0.0, 0.0, 0.0)),
                Text::new("也可以把压缩包拖到窗口任意位置")
                    .alignment(TextAlignment::Center)
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted())
                    .padding_insets(EdgeInsets::new(20.0, 0.0, 0.0, 0.0)),
                Spacer::new(0.0),
            )),
        )
        .fill(),
    )
}

/// A generous white card: glyph badge, one bold line, one quiet line.
fn action_card(
    glyph: cove_ui::imaging::BundleImageResource,
    primary: bool,
    title: &str,
    subtitle: impl Into<String>,
    action: impl Fn() + 'static,
) -> AnyView {
    let badge_tint = if primary {
        SemanticColorRef::accent()
    } else {
        SemanticColorRef::on_surface_variant()
    };
    let badge_bg = if primary {
        SemanticColorRef::accent_muted()
    } else {
        SemanticColorRef::surface_variant()
    };
    let content = Column::new(
        HorizontalAlignment::Center,
        0.0,
        AnyView::new(views!(
            tile(glyph, 44.0, badge_bg, badge_tint),
            Text::new(title)
                .alignment(TextAlignment::Center)
                .semantic_font(SemanticFontRef::headline())
                .semantic_foreground(SemanticColorRef::on_surface())
                .padding_insets(EdgeInsets::new(14.0, 0.0, 0.0, 0.0)),
            Text::new(subtitle.into())
                .alignment(TextAlignment::Center)
                .line_limit(Some(1))
                .truncating_tail()
                .semantic_font(SemanticFontRef::caption())
                .semantic_foreground(SemanticColorRef::on_surface_muted())
                .padding_insets(EdgeInsets::new(5.0, 0.0, 0.0, 0.0)),
        )),
    )
    .padding_insets(EdgeInsets::new(28.0, 14.0, 20.0, 14.0));

    AnyView::new(
        Button::new(content, action)
            .accessibility_text(title.to_string())
            .button_style(PlainButtonStyle)
            .frame_size(286.0, 152.0)
            .corner_radius(12.0)
            .semantic_background(SemanticColorRef::surface())
            .semantic_border(SemanticColorRef::border(), 1.0)
            .shadow(Color::from_rgba8(0, 0, 0, 10), 0.0, 2.0, 10.0),
    )
}

fn reading() -> AnyView {
    AnyView::new(
        Column::new(
            HorizontalAlignment::Center,
            10.0,
            AnyView::new(views!(
                Spinner::new().size(22.0),
                Text::new("正在读取归档…")
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )),
        )
        .fill(),
    )
}

// ---------------------------------------------------------------------------
// pieces
// ---------------------------------------------------------------------------

fn tile(
    glyph: cove_ui::imaging::BundleImageResource,
    size: f32,
    background: SemanticColorRef,
    tint: SemanticColorRef,
) -> AnyView {
    let radius = if size >= 60.0 {
        18.0
    } else if size >= 40.0 {
        12.0
    } else {
        8.0
    };
    AnyView::new(
        AnyView::new(
            AnyView::new(ButtonIcon::new(
                ButtonIconSource::resource(glyph),
                Some(size * 0.48),
                None,
            ))
            .semantic_foreground(tint),
        )
        .frame_size(size, size)
        .corner_radius(radius)
        .semantic_background(background),
    )
}

fn file_glyph(ext: &str, folder: bool) -> (cove_ui::imaging::BundleImageResource, SemanticColorRef) {
    if folder {
        return (file::folder(), SemanticColorRef::accent());
    }
    match ext.to_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "heic" | "bmp" | "ico" => {
            (media::image(), SemanticColorRef::accent_secondary())
        }
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => {
            (media::music(), SemanticColorRef::accent_secondary())
        }
        "mp4" | "mov" | "mkv" | "avi" | "webm" => {
            (media::video(), SemanticColorRef::accent_secondary())
        }
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" => {
            (file::package(), SemanticColorRef::accent())
        }
        _ => (file::file(), SemanticColorRef::on_surface_muted()),
    }
}

fn compare_text(left: &str, right: &str) -> ComparisonResult {
    compare_ordering(left.to_lowercase().cmp(&right.to_lowercase()))
}

fn compare_number(left: u64, right: u64) -> ComparisonResult {
    compare_ordering(left.cmp(&right))
}

fn compare_ordering(ordering: Ordering) -> ComparisonResult {
    match ordering {
        Ordering::Less => ComparisonResult::Ascending,
        Ordering::Equal => ComparisonResult::Same,
        Ordering::Greater => ComparisonResult::Descending,
    }
}
