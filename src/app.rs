use std::path::PathBuf;
use std::rc::Rc;

use cove_ui::ApplicationLaunch;
use cove_ui::compose::adaptive::SurfaceEnvironment;
use cove_ui::compose::theme::ThemeViewExt;
use cove_ui::composition::{AnyView, DynamicField, Scope, View};
use cove_ui::hosts::shell_services::{self, ShellVerb};

use crate::compress_service;
use crate::extract_service;
use crate::hash_service::{self, Algorithm};
use crate::menu_bar::menu_bar;
use crate::model::AppModel;
use crate::theme;
use crate::views::RootView;

pub fn application() -> ApplicationLaunch {
    crate::widget_host::record_main_pid();
    let model = Rc::new(AppModel::new());
    let root = RootView {
        model: model.clone(),
    };
    let services = model.clone();
    let _ = shell_services::install(
        &[
            ShellVerb {
                id: "open",
                label: Some("在 MugenZip 打开"),
                selector: "coveOpenService:",
            },
            ShellVerb {
                id: "extract-here",
                label: Some("解压到当前目录"),
                selector: "coveExtractHereService:",
            },
            ShellVerb {
                id: "extract-folder",
                label: Some("解压到同名目录"),
                selector: "coveExtractFolderService:",
            },
            ShellVerb {
                id: "smart-extract",
                label: Some("智能解压到同名目录"),
                selector: "coveSmartExtractService:",
            },
            ShellVerb {
                id: "compress-zip",
                label: Some("压缩至 ZIP"),
                selector: "coveCompressZipService:",
            },
            ShellVerb {
                id: "compress-7z",
                label: Some("压缩至 7Z"),
                selector: "coveCompressSevenZService:",
            },
            ShellVerb {
                id: "compress-targz",
                label: Some("压缩至 TAR.GZ"),
                selector: "coveCompressTarGzService:",
            },
            ShellVerb {
                id: "compress-ui",
                label: Some("在 MugenZip 中压缩"),
                selector: "coveCompressPanelService:",
            },
            ShellVerb {
                id: "hash-crc32",
                label: Some("校验 CRC32"),
                selector: "coveHashCrc32Service:",
            },
            ShellVerb {
                id: "hash-md5",
                label: Some("校验 MD5"),
                selector: "coveHashMd5Service:",
            },
            ShellVerb {
                id: "hash-sha1",
                label: Some("校验 SHA-1"),
                selector: "coveHashSha1Service:",
            },
            ShellVerb {
                id: "hash-sha256",
                label: Some("校验 SHA-256"),
                selector: "coveHashSha256Service:",
            },
            ShellVerb {
                id: "hash-sha512",
                label: Some("校验 SHA-512"),
                selector: "coveHashSha512Service:",
            },
        ],
        move |verb, paths| {
            /* "open" wants the window; the in-place verbs do not, so they work
               headlessly and hand the app straight back to whatever was in front. */
            if verb.starts_with("extract") || verb.starts_with("hash") {
                shell_services::yield_to_background();
            }
            let mut compress_paths: Vec<PathBuf> = Vec::new();
            let mut handoff_paths: Vec<PathBuf> = Vec::new();
            for path in paths {
                let path = PathBuf::from(path);
                match verb {
                    "extract-here" => {
                        extract_service::run(path, extract_service::Placement::Here)
                    }
                    "extract-folder" => {
                        extract_service::run(path, extract_service::Placement::IntoFolder)
                    }
                    "smart-extract" => {
                        extract_service::run(path, extract_service::Placement::Smart)
                    }
                    "compress-zip" | "compress-7z" | "compress-targz" | "compress-here" => {
                        compress_paths.push(path)
                    }
                    "compress-ui" => handoff_paths.push(path),
                    "hash-crc32" => hash_async(path, Algorithm::Crc32),
                    "hash-md5" => hash_async(path, Algorithm::Md5),
                    "hash-sha1" => hash_async(path, Algorithm::Sha1),
                    "hash-sha256" => hash_async(path, Algorithm::Sha256),
                    "hash-sha512" => hash_async(path, Algorithm::Sha512),
                    _ => {
                        if !services.is_busy() {
                            services.open(path);
                        }
                    }
                }
            }
            if !compress_paths.is_empty() {
                let format = match verb {
                    "compress-7z" => mugenzip_core::CreateFormat::SevenZ,
                    "compress-targz" => mugenzip_core::CreateFormat::TarGz,
                    "compress-zip" => mugenzip_core::CreateFormat::Zip,
                    _ => services.default_create_format.get(),
                };
                compress_service::run(
                    compress_paths,
                    format,
                    services.default_create_level.get(),
                );
            }
            if !handoff_paths.is_empty() {
                services.open_create_with(handoff_paths);
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

fn hash_async(path: PathBuf, algorithm: Algorithm) {
    std::thread::spawn(move || {
        let _ = hash_service::write_report(&path, algorithm);
    });
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
