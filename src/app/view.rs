use std::cmp::Ordering;
use std::rc::Rc;

use cove_ui::compose::modifiers::layout::EdgeInsets;
use cove_ui::compose::modifiers::style::AppearanceExt;
use cove_ui::compose::prelude::*;
use cove_ui::compose::primitives::data::table::{ColumnVisibility, ComparisonResult};
use cove_ui::compose::primitives::media::ui_icon_catalog::{action, file, media, navigation, status};
use cove_ui::compose::theme::styles::buttons::{
    ButtonStyleViewExt, GhostButtonStyle, PrimaryButtonStyle, SecondaryButtonStyle,
};
use cove_ui::composition::{
    DynamicField, EmptyView, HorizontalAlignment, Spacer, VerticalAlignment, views,
};

use super::browser::FileItem;
use super::model::{AppModel, ARCHIVE_EXTENSIONS, Phase, Tone};
use crate::engine::format_size;

const DETAIL_WIDTH: f32 = 280.0;
const CARD_RADIUS: f32 = 16.0;

pub struct RootView {
    pub model: Rc<AppModel>,
}

impl View for RootView {
    fn body(&self, _cx: &Scope) -> AnyView {
        self.model.ensure_platform_hooks();
        let model = self.model.clone();

        let shell = Column::new(
            HorizontalAlignment::Leading,
            0.0,
            AnyView::new(views!(
                Column::new(
                    HorizontalAlignment::Leading,
                    14.0,
                    AnyView::new(views!(
                        header(model.clone()),
                        content(model.clone()),
                        extraction(model.clone()),
                    )),
                )
                .padding_insets(EdgeInsets::new(16.0, 16.0, 12.0, 16.0))
                .flex(1.0),
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

fn header(model: Rc<AppModel>) -> AnyView {
    let busy = model.is_busy();
    let loaded = model.archive.get();
    let open = model.clone();
    let extract = model.clone();

    let (chip, title, subtitle) = match &loaded {
        Some(archive) => (
            icon_chip(
                file::archive(),
                SemanticColorRef::accent(),
                SemanticColorRef::accent_muted(),
                22.0,
                11.0,
                12.0,
            ),
            AnyView::new(
                Row::new(
                    VerticalAlignment::Center,
                    8.0,
                    AnyView::new(views!(
                        Text::new(archive.name.clone())
                            .truncating_tail()
                            .semantic_font(SemanticFontRef::headline())
                            .semantic_foreground(SemanticColorRef::on_surface())
                            .frame_min_width(0.0)
                            .frame(None, None, None, None, Some(420.0), None),
                        Badge::new(archive.format).tone(StatusTone::Info),
                    )),
                ),
            ),
            format!(
                "{} 个项目 · {}",
                archive.entry_count,
                format_size(archive.total_bytes)
            ),
        ),
        None => (
            icon_chip(
                file::inbox(),
                SemanticColorRef::on_surface_muted(),
                SemanticColorRef::surface_variant(),
                22.0,
                11.0,
                12.0,
            ),
            AnyView::new(
                Text::new("MugenZip")
                    .semantic_font(SemanticFontRef::headline())
                    .semantic_foreground(SemanticColorRef::on_surface()),
            ),
            "打开或拖入压缩包开始浏览".to_string(),
        ),
    };

    let mut actions: Vec<AnyView> = Vec::new();
    if loaded.is_some() {
        actions.push(AnyView::new(
            labelled_button(action::download(), "解压到…", move || extract.extract_dialog())
                .enabled(!busy)
                .button_style(PrimaryButtonStyle),
        ));
        actions.push(AnyView::new(
            labelled_button(file::archive(), "打开", move || open.open_dialog())
                .enabled(!busy)
                .button_style(SecondaryButtonStyle),
        ));
    } else {
        actions.push(AnyView::new(
            labelled_button(file::archive(), "打开压缩包", move || open.open_dialog())
                .enabled(!busy)
                .button_style(PrimaryButtonStyle),
        ));
    }

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            12.0,
            AnyView::new(views!(
                chip,
                Column::new(
                    HorizontalAlignment::Leading,
                    3.0,
                    AnyView::new(views!(
                        title,
                        Text::new(subtitle)
                            .semantic_font(SemanticFontRef::caption())
                            .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    )),
                )
                .frame_min_width(0.0)
                .flex(1.0),
                TupleView::new(actions),
            )),
        )
        .fill_width(),
    )
}

fn content(model: Rc<AppModel>) -> AnyView {
    let body = match (model.archive.get(), model.phase.get()) {
        (None, Phase::Loading) => AnyView::new(
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
        ),
        (None, _) => {
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
                .fill()
                .semantic_background(SemanticColorRef::surface())
                .corner_radius(CARD_RADIUS)
                .semantic_border(SemanticColorRef::border(), 1.0),
            )
        }
        (Some(_), _) => AnyView::new(
            Row::new(
                VerticalAlignment::Top,
                12.0,
                AnyView::new(views!(
                    Column::new(
                        HorizontalAlignment::Leading,
                        8.0,
                        AnyView::new(views!(path_row(model.clone()), file_table(model.clone()),)),
                    )
                    .frame_min_width(0.0)
                    .flex(1.0)
                    .fill_height(),
                    detail_panel(model.clone()),
                )),
            )
            .fill(),
        ),
    };
    AnyView::new(body.flex(1.0))
}

fn path_row(model: Rc<AppModel>) -> AnyView {
    let directory = model.directory.get();
    let crumbs = model
        .archive
        .get()
        .map(|archive| archive.browser.breadcrumbs(&directory))
        .unwrap_or_default();
    let items = crumbs
        .iter()
        .map(|crumb| BreadcrumbItem::new(crumb.path.clone(), crumb.name.clone()))
        .collect();
    let select = model.clone();
    let breadcrumb = Breadcrumb::try_new(items, 5, move |path| select.enter_directory(path))
        .expect("breadcrumb paths are unique by construction");
    let up = model.clone();

    AnyView::new(
        Row::new(
            VerticalAlignment::Center,
            6.0,
            AnyView::new(views!(
                Button::icon_default(
                    ButtonIconSource::resource(navigation::arrow_up()),
                    move || up.navigate_up(),
                )
                .enabled(!directory.is_empty())
                .accessibility_text("上一级")
                .tooltip("上一级")
                .button_style(GhostButtonStyle),
                breadcrumb,
            )),
        )
        .fill_width(),
    )
}

fn file_table(model: Rc<AppModel>) -> AnyView {
    let mut options = TableOptions::default();
    options.selection = Some(Selection::single(model.selection.binding()));
    options.frozen_column_count = 1;
    options.layout = TableLayout {
        row_height: 36.0,
        header_height: 36.0,
        is_striped: false,
        separator_inset: Some(0.0),
        fills_width: true,
        empty_title: "此目录为空".into(),
    };

    let activate = model.clone();
    let table = Table::new(model.rows(), |row: &FileItem| row.id.clone(), columns())
        .expect("the file table columns are static with unique ids")
        .options(options)
        .on_activate(move |row: &FileItem| activate.activate(row))
        .debug_name("file-table");
    AnyView::new(table.flex(1.0).fill_height())
}

fn columns() -> Vec<TableColumn<FileItem>> {
    vec![
        TableColumn::new("name", "名称", |row: &FileItem| {
            let (glyph, tint) = file_glyph(&row.ext, row.folder);
            let font = if row.folder {
                SemanticFontRef::body_strong()
            } else {
                SemanticFontRef::body()
            };
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
                .frame_min_width(0.0),
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

fn detail_panel(model: Rc<AppModel>) -> AnyView {
    let content = match model.selected_item() {
        Some(item) => detail_content(&item),
        None => AnyView::new(
            Column::new(
                HorizontalAlignment::Center,
                10.0,
                AnyView::new(views!(
                    Spacer::new(0.0),
                    AnyView::new(ButtonIcon::new(
                        ButtonIconSource::resource(file::file_text()),
                        Some(28.0),
                        None,
                    ))
                    .semantic_foreground(SemanticColorRef::on_surface_disabled()),
                    Text::new("选择文件查看详情")
                        .semantic_font(SemanticFontRef::caption())
                        .semantic_foreground(SemanticColorRef::on_surface_muted()),
                    Spacer::new(0.0),
                )),
            )
            .fill(),
        ),
    };
    AnyView::new(
        Column::new(HorizontalAlignment::Leading, 0.0, content)
            .frame_width(DETAIL_WIDTH)
            .fill_height()
            .semantic_background(SemanticColorRef::surface())
            .corner_radius(CARD_RADIUS)
            .semantic_border(SemanticColorRef::border(), 1.0)
            .clipped(),
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

fn extraction(model: Rc<AppModel>) -> AnyView {
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
        .padding_insets(EdgeInsets::new(12.0, 14.0, 12.0, 14.0))
        .fill_width()
        .semantic_background(SemanticColorRef::surface())
        .corner_radius(12.0)
        .semantic_border(SemanticColorRef::border(), 1.0),
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
        Some(archive) => {
            let items = archive.browser.items(&model.directory.get());
            let folders = items.iter().filter(|item| item.folder).count();
            let files = items.len() - folders;
            format!("{folders} 个文件夹 · {files} 个文件")
        }
        None => "拖放压缩包到窗口即可打开".to_string(),
    };
    let trailing = StatusBarText::new(summary);

    AnyView::new(StatusBar::new(
        AnyView::new(leading),
        AnyView::new(trailing),
    ))
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