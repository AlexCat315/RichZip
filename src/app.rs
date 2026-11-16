use std::path::PathBuf;
use std::rc::Rc;

use cove_ui::ApplicationLaunch;
use cove_ui::compose::adaptive::SurfaceEnvironment;
use cove_ui::compose::theme::ThemeViewExt;
use cove_ui::composition::{AnyView, DynamicField, Scope, View};
use cove_ui::hosts::shell_services::{self, ShellVerb};

use crate::compress_service;
use crate::extract_service;
use crate::menu_bar::menu_bar;
use crate::model::AppModel;
use crate::theme;
use crate::views::RootView;

pub fn application() -> ApplicationLaunch {
    let model = Rc::new(AppModel::new());
    let root = RootView {
        model: model.clone(),
    };
    let services = model.clone();
    let _ = shell_services::install(
        &[
            ShellVerb {
                id: "open",
                label: Some("用 MugenZip 打开"),
                selector: "coveOpenService:",
            },
            ShellVerb {
                id: "extract-here",
                label: Some("解压到此处"),
                selector: "coveExtractHereService:",
            },
            ShellVerb {
                id: "extract-folder",
                label: Some("解压到同名文件夹"),
                selector: "coveExtractFolderService:",
            },
            ShellVerb {
                id: "compress-here",
                label: Some("压缩到此处"),
                selector: "coveCompressHereService:",
            },
        ],
        move |verb, paths| {
            /* "open" wants the window; the in-place verbs do not, so they work
               headlessly and hand the app straight back to whatever was in front. */
            if matches!(verb, "extract-here" | "extract-folder" | "compress-here") {
                shell_services::yield_to_background();
            }
            let mut compress_paths: Vec<PathBuf> = Vec::new();
            for path in paths {
                let path = PathBuf::from(path);
                match verb {
                    "extract-here" => extract_service::run(path, extract_service::Placement::Here),
                    "extract-folder" => extract_service::run(path, extract_service::Placement::IntoFolder),
                    "compress-here" => compress_paths.push(path),
                    _ => {
                        if !services.is_busy() {
                            services.open(path);
                        }
                    }
                }
            }
            if !compress_paths.is_empty() {
                compress_service::run(
                    compress_paths,
                    services.default_create_format.get(),
                    services.default_create_level.get(),
                );
            }
        },
    );
    // The menu-bar extra belongs to MugenZipWidget.app. Keeping a second
    // status item here would put two archive boxes in the menu bar whenever
    // the dot helper is running.
    let themed = model.clone();
    ApplicationLaunch::new(root)
        .configure(|app| {
            app.window.title = "MugenZip".into();
            app.window.width = 1180;
            app.window.height = 760;
            app.window.min_width = 940;
            app.window.min_height = 620;
        })
        .wrap_content(move |content| ThemedContent {
            model: themed.clone(),
            content,
        })
        .menu_bar(menu_bar(model))
        .diagnostics(cove_ui::hosts::desktop::diagnostics::devtools::factory())
}

/// App-wide theme provider, mounted for the whole window chrome and content.
struct ThemedContent {
    model: Rc<AppModel>,
    content: AnyView,
}

impl View for ThemedContent {
    fn body(&self, cx: &Scope) -> AnyView {
        let env = cx.composition_value(&SurfaceEnvironment::key());
        let theme = theme::resolve(self.model.theme_mode(), env.system_appearance.is_dark());
        AnyView::new(AnyView::new(self.content.clone()).theme(theme))
    }

    fn dynamic_properties(&self) -> Vec<DynamicField> {
        vec![DynamicField::state("themeMode", &self.model.theme_mode)]
    }
}
