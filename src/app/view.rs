use std::cmp::Ordering;
use std::rc::Rc;
use std::sync::Arc;

use cove_ui::compose::modifiers::layout::EdgeInsets;
use cove_ui::compose::modifiers::style::AppearanceExt;
use cove_ui::compose::prelude::*;
use cove_ui::compose::primitives::data::lists::tree::TreeOptions;
use cove_ui::compose::primitives::data::table::{ColumnVisibility, ComparisonResult};
use cove_ui::compose::primitives::media::ui_icon_catalog::{action, file, layout, media, navigation, status};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, GhostButtonStyle, PrimaryButtonStyle,
};
use cove_ui::composition::{
    DynamicField, EmptyView, HorizontalAlignment, Spacer, VerticalAlignment, views,
};

use super::browser::FileItem;
use super::model::{AppModel, ARCHIVE_EXTENSIONS, LoadedArchive, Phase, Tone};
use crate::engine::{archive_base_name, format_size};

pub struct RootView {
    pub model: Rc<AppModel>,
}

impl View for RootView {
    fn body(&self, _cx: &Scope) -> AnyView {
        self.model.ensure_platform_hooks();
        let model = self.model.clone();

        let main = match (model.archive.get(), model.phase.get()) {
            (None, Phase::Loading) => loading_view(),
            (None, _) => empty_view(model.clone()),
            (Some(archive), _) => workspace_view(model.clone(), archive),
        };

        let shell = Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                toolbar(model.clone()),
                nav_bar(model.clone()),
                main.flex(1.0),
                extraction_bar(model.clone()),
                status_bar(model.clone()),
            )),
        )
        .fill()
        .semantic_background(SemanticColorRef::background());

        let drop = model.clone();
        AnyView::new(
            FileDropZone::new(move |files| drop.open_dropped(files), shell)
                .accepted_extensions(ARCHIVE_EXTENSIONS)
                .maximum_files(1)
                .shows_chrome(false)
                .is_targeted(model.drop_targeted.binding())
                .fill(),
        )
    }

    fn dynamic_properties(&self) -> Vec<DynamicField> {
        self.model.dynamic_fields()
    }
}

fn toolbar(model: Rc<AppModel>) -> AnyView {
    let busy = model.is_busy();
    let loaded = model.archive.get().is_some();

    let open = model.clone();
    let open_button = labelled_button(file::archive(), "打开", move || open.open_dialog())
        .enabled(!busy);

    let smart = model.clone();
    let extract = SplitButton::new("智能解压", extract_menu(model.clone()), move || {
        smart.extract_smart()
    })
    .enabled(!busy && loaded)
    .menu_accessibility_label("解压选项");

    let search = SearchField::new(model.search.binding())
        .placeholder("搜索当前目录")
        .frame_width(220.0);

    AnyView::new(
        Toolbar::new(Row::new(
            VerticalAlignment::Center,
            10.0,
            AnyView::new(views!(
                ToolbarGroup::new(open_button),
                ToolbarSeparator,
                ToolbarGroup::new(extract).bordered(true),
                Spacer::new(0.0),
                search,
            )),
        ))
        .minimum_height(52.0)
        .vertical_padding(8.0)
        .fill_width()
        .semantic_background(SemanticColorRef::surface()),
    )
}

fn extract_menu(model: Rc<AppModel>) -> Vec<MenuEntry> {
    let named_label = match model.archive.get() {
        Some(archive) => format!("解压到「{}」", archive_base_name(&archive.path)),
        None => "解压到名称文件夹".into(),
    };
    let smart = model.clone();
    let here = model.clone();
    let named = model.clone();
    let dialog = model.clone();
    vec![
        MenuEntry::Item(MenuItem::new(1, "智能解压", move || smart.extract_smart())),
        MenuEntry::Item(MenuItem::new(2, "解压到当前文件夹", move || here.extract_here())),
        MenuEntry::Item(MenuItem::new(3, named_label, move || named.extract_named())),
        MenuEntry::separator(4),
        MenuEntry::Item(MenuItem::new(5, "解压到…", move || dialog.extract_dialog())),
    ]
}

fn nav_bar(model: Rc<AppModel>) -> AnyView {
    let directory = model.directory.get();
    let loaded = model.archive.get().is_some();

    let up = model.clone();
    let up_button = Button::icon_default(
        ButtonIconSource::resource(navigation::arrow_up()),
        move || up.navigate_up(),
    )
    .enabled(loaded && !directory.is_empty())
    .accessibility_text("上一级")
    .tooltip("上一级")
    .button_style(GhostButtonStyle);

    let breadcrumb: AnyView = match model.archive.get() {
        Some(archive) => {
            let crumbs = archive.browser.breadcrumbs(&directory);
            let items = crumbs
                .iter()
                .map(|crumb| BreadcrumbItem::new(crumb.path.clone(), crumb.name.clone()))
                .collect();
            let select = model.clone();
            AnyView::new(
                Breadcrumb::try_new(items, 6, move |path| select.enter_directory(path))
                    .expect("breadcrumb paths are unique by construction"),
            )
        }
        None => AnyView::new(
            Text::new("未打开压缩包")
                .semantic_font(SemanticFontRef::body())
                .semantic_foreground(SemanticColorRef::on_surface_muted()),
        ),
    };

    let toggle = model.clone();
    let inspector_visible = model.inspector_visible.get();
    let inspector_button = Button::icon_default(
        ButtonIconSource::resource(layout::panel_right()),
        move || toggle.toggle_inspector(),
    )
    .enabled(loaded)
    .accessibility_text("显示或隐藏简介")
    .tooltip(if inspector_visible { "隐藏简介" } else { "显示简介" })
    .button_style(GhostButtonStyle);

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            6.0,
            AnyView::new(views!(
                up_button,
                breadcrumb,
                Spacer::new(0.0),
                inspector_button,
            )),
        )
        .fill_width()
        .padding_insets(EdgeInsets::new(6.0, 12.0, 6.0, 12.0)),
    )
}

fn workspace_view(model: Rc<AppModel>, archive: Arc<LoadedArchive>) -> AnyView {
    let sidebar = sidebar_tree(model.clone(), archive);
    let table = file_table(model.clone());
    let content = if model.inspector_visible.get() {
        let inspector = inspector_pane(model.clone());
        AnyView::new(SplitView::new(SplitAxis::Horizontal, 0.72, table, inspector))
    } else {
        AnyView::new(table)
    };
    AnyView::new(
        SplitView::resizable(
            SplitAxis::Horizontal,
            model.sidebar_fraction.binding(),
            sidebar,
            content,
        )
        .constraints(SplitConstraints::new(0.15, 0.40).minimums(180.0, 480.0))
        .flex(1.0)
        .fill_width(),
    )
}

fn sidebar_tree(model: Rc<AppModel>, archive: Arc<LoadedArchive>) -> AnyView {
    let roots = vec![archive.browser.archive_root_item()];
    let children_archive = archive.clone();
    let nav = model.clone();

    let mut options = TreeOptions::<FileItem, String>::default();
    options.selection.ids = Selection::single(model.tree_selection.binding());
    options.expansion.default_expand_all = true;
    options.expansion.auto_expand_parent = true;
    options.layout.row_height = 28.0;
    options.layout.indentation = 14.0;
    options.layout.leading_padding = 8.0;
    options.slots.icon = Some(Rc::new(|item: &FileItem| {
        let glyph = if item.id.is_empty() {
            file::archive()
        } else {
            file::folder()
        };
        AnyView::new(
            AnyView::new(ButtonIcon::new(
                ButtonIconSource::resource(glyph),
                Some(15.0),
                None,
            ))
            .semantic_foreground(SemanticColorRef::accent()),
        )
    }));
    options.events.on_select = Some(Rc::new(move |item: FileItem| nav.enter_directory(item.id)));

    let tree = Tree::new(
        roots,
        |item: &FileItem| item.id.clone(),
        move |item: &FileItem| children_archive.browser.subfolders(&item.id),
        |item: &FileItem, _state| {
            AnyView::new(
                Text::new(item.name.clone())
                    .truncating_tail()
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface()),
            )
        },
    )
    .options(options);

    AnyView::new(
        Column::new(HorizontalAlignment::Leading, 0.0, AnyView::new(tree.flex(1.0)))
            .fill_height()
            .semantic_background(SemanticColorRef::surface_variant()),
    )
}

fn file_table(model: Rc<AppModel>) -> AnyView {
    let mut options = TableOptions::default();
    options.selection = Some(Selection::multiple(model.selection.binding()));
    options.sort = Some(model.table_sort.binding());
    options.frozen_column_count = 1;
    options.layout = TableLayout {
        row_height: 34.0,
        header_height: 32.0,
        is_striped: false,
        separator_inset: Some(0.0),
        fills_width: true,
        empty_title: "此目录为空".into(),
    };

    let activate = model.clone();
    let table = Table::new(model.rows(), |row: &FileItem| row.id.clone(), columns(model.clone()))
        .expect("the file table columns are static with unique ids")
        .options(options)
        .on_activate(move |row: &FileItem| activate.activate(row))
        .debug_name("file-table");
    AnyView::new(table.flex(1.0).fill_height())
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
                    10.0,
                    AnyView::new(views!(
                        AnyView::new(ButtonIcon::new(
                            ButtonIconSource::resource(glyph),
                            Some(16.0),
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
            min_width: 200.0,
            max_width: 2000.0,
            is_flex: true,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_text(&left.name, &right.name)),
        TableColumn::new("size", "大小", |row: &FileItem| {
            AnyView::new(
                Text::new(row.size.clone())
                    .semantic_font(SemanticFontRef::mono())
                    .semantic_foreground(SemanticColorRef::on_surface_variant()),
            )
        })
        .layout(TableColumnLayout {
            width: 100.0,
            alignment: TextAlignment::Trailing,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_number(left.bytes, right.bytes)),
        TableColumn::new("modified", "修改日期", |row: &FileItem| {
            AnyView::new(
                Text::new(row.modified.clone())
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )
        })
        .layout(TableColumnLayout {
            width: 150.0,
            ..TableColumnLayout::default()
        })
        .visibility(ColumnVisibility::Required)
        .compare(|left, right| compare_text(&left.modified, &right.modified)),
    ]
}

fn row_menu(model: Rc<AppModel>, id: String) -> Vec<MenuEntry> {
    let smart = model.clone();
    let smart_id = id.clone();
    let here = model.clone();
    let here_id = id.clone();
    let copy = model.clone();
    let copy_id = id.clone();
    let props = model.clone();
    let props_id = id.clone();
    vec![
        MenuEntry::Item(MenuItem::new(1, "智能解压", move || {
            smart.extract_item_smart(smart_id.clone())
        })),
        MenuEntry::Item(MenuItem::new(2, "解压到当前文件夹", move || {
            here.extract_item_here(here_id.clone())
        })),
        MenuEntry::separator(3),
        MenuEntry::Item(MenuItem::new(4, "复制路径", move || copy.copy_path(copy_id.clone()))),
        MenuEntry::Item(MenuItem::new(5, "显示简介", move || {
            props.show_properties(props_id.clone())
        })),
    ]
}

fn inspector_pane(model: Rc<AppModel>) -> AnyView {
    let selected = model.selected_items();
    let content = match selected.len() {
        0 => archive_overview(model.clone()),
        1 => detail_content(&selected[0]),
        _ => multi_summary(&selected),
    };
    AnyView::new(
        Column::new(HorizontalAlignment::Leading, 0.0, content)
            .fill_height()
            .semantic_background(SemanticColorRef::surface()),
    )
}

fn archive_overview(model: Rc<AppModel>) -> AnyView {
    let Some(archive) = model.archive.get() else {
        return AnyView::new(EmptyView);
    };
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                Column::new(
                    HorizontalAlignment::Center,
                    10.0,
                    AnyView::new(views!(
                        icon_chip(
                            file::archive(),
                            SemanticColorRef::accent(),
                            SemanticColorRef::accent_muted(),
                            30.0,
                            17.0,
                            16.0,
                        ),
                        Text::new(archive.name.clone())
                            .line_limit(Some(3))
                            .alignment(TextAlignment::Center)
                            .semantic_font(SemanticFontRef::body_strong())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        Badge::new(archive.format).tone(StatusTone::Info),
                    )),
                )
                .padding_insets(EdgeInsets::new(24.0, 20.0, 18.0, 20.0))
                .fill_width(),
                Divider::horizontal(),
                Column::new(
                    HorizontalAlignment::Leading,
                    14.0,
                    AnyView::new(views!(
                        property("项目数", archive.entry_count.to_string(), false),
                        property("总大小", format_size(archive.total_bytes), false),
                    )),
                )
                .padding(18.0)
                .fill_width(),
                Spacer::new(0.0),
                Text::new("选择文件查看详情")
                    .alignment(TextAlignment::Center)
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted())
                    .padding(18.0)
                    .fill_width(),
            )),
        )
        .fill(),
    )
}

fn multi_summary(items: &[FileItem]) -> AnyView {
    let total: u64 = items.iter().map(|item| item.bytes).sum();
    let folders = items.iter().filter(|item| item.folder).count();
    let files = items.len() - folders;
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                Column::new(
                    HorizontalAlignment::Center,
                    10.0,
                    AnyView::new(views!(
                        icon_chip(
                            file::folder(),
                            SemanticColorRef::accent(),
                            SemanticColorRef::accent_muted(),
                            30.0,
                            17.0,
                            16.0,
                        ),
                        Text::new(format!("{} 个项目", items.len()))
                            .semantic_font(SemanticFontRef::body_strong())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        Text::new("已选中")
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    )),
                )
                .padding_insets(EdgeInsets::new(24.0, 20.0, 18.0, 20.0))
                .fill_width(),
                Divider::horizontal(),
                Column::new(
                    HorizontalAlignment::Leading,
                    14.0,
                    AnyView::new(views!(
                        property("文件夹", folders.to_string(), false),
                        property("文件", files.to_string(), false),
                        property("总大小", format_size(total), false),
                    )),
                )
                .padding(18.0)
                .fill_width(),
            )),
        )
        .fill(),
    )
}

fn detail_content(item: &FileItem) -> AnyView {
    let (glyph, tint) = file_glyph(&item.ext, item.folder);
    let kind = kind_label(item);
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                Column::new(
                    HorizontalAlignment::Center,
                    10.0,
                    AnyView::new(views!(
                        icon_chip(
                            glyph,
                            tint,
                            SemanticColorRef::surface_variant(),
                            30.0,
                            17.0,
                            16.0,
                        ),
                        Text::new(item.name.clone())
                            .line_limit(Some(3))
                            .alignment(TextAlignment::Center)
                            .semantic_font(SemanticFontRef::body_strong())
                            .semantic_foreground(SemanticColorRef::on_surface()),
                        Text::new(kind.clone())
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    )),
                )
                .padding_insets(EdgeInsets::new(24.0, 20.0, 18.0, 20.0))
                .fill_width(),
                Divider::horizontal(),
                Column::new(
                    HorizontalAlignment::Leading,
                    14.0,
                    AnyView::new(views!(
                        property("类型", kind, false),
                        property("大小", item.size.clone(), false),
                        property("修改时间", item.modified.clone(), false),
                        property("路径", item.path.clone(), true),
                    )),
                )
                .padding(18.0)
                .fill_width(),
            )),
        )
        .fill(),
    )
}

fn loading_view() -> AnyView {
    AnyView::new(
        Column::new(
            HorizontalAlignment::Center,
            12.0,
            AnyView::new(views!(
                Spinner::new().size(28.0),
                Text::new("正在读取归档…")
                    .semantic_font(SemanticFontRef::body())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
            )),
        )
        .fill(),
    )
}

fn empty_view(model: Rc<AppModel>) -> AnyView {
    let open = model.clone();
    AnyView::new(
        EmptyState::with_action(
            "没有打开的压缩包",
            "把压缩包拖进窗口，或从工具栏选择一个文件。",
            labelled_button(file::archive(), "打开压缩包", move || open.open_dialog())
                .button_style(PrimaryButtonStyle),
        )
        .icon(file::inbox())
        .prominence(EmptyStateProminence::Spacious)
        .fill(),
    )
}

fn extraction_bar(model: Rc<AppModel>) -> AnyView {
    let progress = model.progress.get();
    if model.phase.get() != Phase::Extracting && progress.fraction <= 0.0 {
        return AnyView::new(EmptyView);
    }
    let cancel = model.clone();
    let extracting = model.phase.get() == Phase::Extracting;
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            8.0,
            AnyView::new(views!(
                Row::new(
                    VerticalAlignment::Center,
                    10.0,
                    AnyView::new(views!(
                        AnyView::new(ButtonIcon::new(
                            ButtonIconSource::resource(action::download()),
                            Some(15.0),
                            None,
                        ))
                        .semantic_foreground(SemanticColorRef::accent()),
                        Text::new(progress.current_file.clone())
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_variant())
                            .frame_min_width(0.0)
                            .flex(1.0),
                        Text::new(progress.label.clone())
                            .semantic_font(SemanticFontRef::mono())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                        Button::icon_default(
                            ButtonIconSource::resource(action::x()),
                            move || cancel.cancel_extract(),
                        )
                        .enabled(extracting)
                        .accessibility_text("取消提取")
                        .tooltip("取消提取")
                        .button_style(GhostButtonStyle),
                    )),
                )
                .fill_width(),
                ProgressView::new(Some(progress.fraction))
                    .height(4.0)
                    .tone(StatusTone::Info)
                    .fill_width(),
            )),
        )
        .padding_insets(EdgeInsets::new(10.0, 14.0, 10.0, 14.0))
        .fill_width()
        .semantic_background(SemanticColorRef::surface()),
    )
}

fn status_bar(model: Rc<AppModel>) -> AnyView {
    let status = model.status.get();
    let (tone, glyph) = match status.tone {
        Tone::Ok => (StatusTone::Success, status::check_circle()),
        Tone::Busy => (StatusTone::Info, status::loader()),
        Tone::Error => (StatusTone::Danger, status::alert_circle()),
    };
    let leading = StatusBarText::new(status.text.clone())
        .detail(status.detail.clone())
        .icon(glyph)
        .tone(tone);

    let summary = match model.archive.get() {
        Some(_) => {
            if model.selection.get().is_empty() {
                let items = model.current_items();
                let folders = items.iter().filter(|item| item.folder).count();
                let files = items.len() - folders;
                format!("{folders} 个文件夹 · {files} 个文件")
            } else {
                let items = model.selected_items();
                let total: u64 = items.iter().map(|item| item.bytes).sum();
                format!("已选 {} 项 · {}", items.len(), format_size(total))
            }
        }
        None => "拖放压缩包到窗口即可打开".to_string(),
    };
    let trailing = StatusBarText::new(summary);

    AnyView::new(StatusBar::new(
        AnyView::new(leading),
        AnyView::new(trailing),
    ))
}

fn property(label: &str, value: String, mono: bool) -> AnyView {
    let font = if mono {
        SemanticFontRef::mono()
    } else {
        SemanticFontRef::body()
    };
    AnyView::new(
        Column::new(
            HorizontalAlignment::Leading,
            3.0,
            AnyView::new(views!(
                Text::new(label)
                    .semantic_font(SemanticFontRef::caption())
                    .semantic_foreground(SemanticColorRef::on_surface_muted()),
                Text::new(value)
                    .line_limit(Some(4))
                    .semantic_font(font)
                    .semantic_foreground(SemanticColorRef::on_surface()),
            )),
        )
        .fill_width(),
    )
}

fn icon_chip(
    glyph: cove_ui::imaging::BundleImageResource,
    tint: SemanticColorRef,
    background: SemanticColorRef,
    icon_size: f32,
    padding: f32,
    radius: f32,
) -> AnyView {
    AnyView::new(
        ButtonIcon::new(ButtonIconSource::resource(glyph), Some(icon_size), None)
            .semantic_foreground(tint)
            .padding(padding)
            .semantic_background(background)
            .corner_radius(radius),
    )
}

fn file_glyph(ext: &str, folder: bool) -> (cove_ui::imaging::BundleImageResource, SemanticColorRef) {
    if folder {
        return (file::folder(), SemanticColorRef::accent());
    }
    match ext.to_lowercase().as_str() {
        "rs" | "c" | "h" | "cpp" | "cc" | "js" | "ts" | "py" | "go" | "swift" | "java" | "sh"
        | "toml" | "json" | "yaml" | "yml" | "xml" | "html" | "css" => {
            (file::file_code(), SemanticColorRef::accent_secondary())
        }
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "heic" | "bmp" | "ico" => {
            (media::image(), SemanticColorRef::success())
        }
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => (media::music(), SemanticColorRef::warning()),
        "mp4" | "mov" | "mkv" | "avi" | "webm" => (media::video(), SemanticColorRef::warning()),
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" => {
            (file::package(), SemanticColorRef::warning())
        }
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => {
            (file::file_text(), SemanticColorRef::error())
        }
        "txt" | "md" | "log" | "rtf" => (file::file_text(), SemanticColorRef::on_surface_variant()),
        _ => (file::file(), SemanticColorRef::on_surface_muted()),
    }
}

fn kind_label(item: &FileItem) -> String {
    if item.folder {
        "文件夹".to_string()
    } else if item.ext.is_empty() {
        "文件".to_string()
    } else {
        format!("{} 文件", item.ext.to_lowercase())
    }
}

fn labelled_button(
    glyph: cove_ui::imaging::BundleImageResource,
    title: &str,
    action: impl Fn() + 'static,
) -> Button {
    Button::new(
        Row::new(
            VerticalAlignment::Center,
            6.0,
            AnyView::new(views!(
                ButtonIcon::new(ButtonIconSource::resource(glyph), Some(15.0), None),
                Text::new(title),
            )),
        ),
        action,
    )
    .accessibility_text(title)
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