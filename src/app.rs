use std::path::PathBuf;
use std::rc::Rc;

use cove_ui::ApplicationLaunch;
use cove_ui::hosts::shell_services::{self, ShellVerb};

use crate::menu_bar::menu_bar;
use crate::model::AppModel;
use crate::views::RootView;

pub fn application() -> ApplicationLaunch {
    let model = Rc::new(AppModel::new());
    let root = RootView {
        model: model.clone(),
    };
    // Finder "Services" entry. The menu item itself is declared by the bundle's
    // `NSServices` (see Templates/app/Platforms/macOS/Info.plist); this only
    // publishes the selector handler. An unpackaged `cargo run` installs it and
    // still shows nothing in Finder, because LaunchServices never sees the
    // binary.
    let services = model.clone();
    let _ = shell_services::install(
        &[ShellVerb {
            id: "open",
            label: Some("用 MugenZip 打开"),
            selector: "coveExtractService:",
        }],
        move |_verb, paths| {
            if let Some(path) = paths.into_iter().next() {
                services.open(PathBuf::from(path));
            }
        },
    );
    ApplicationLaunch::new(root)
        .configure(|app| {
            app.window.title = "MugenZip".into();
            app.window.width = 1180;
            app.window.height = 760;
            app.window.min_width = 940;
            app.window.min_height = 620;
        })
        .menu_bar(menu_bar(model))
        .diagnostics(cove_ui::hosts::desktop::diagnostics::devtools::factory())
}
