use std::path::PathBuf;
use std::rc::Rc;

use cove_ui::ApplicationLaunch;
use cove_ui::compose::adaptive::SurfaceEnvironment;
use cove_ui::compose::theme::ThemeViewExt;
use cove_ui::composition::{AnyView, DynamicField, Scope, View};
use cove_ui::hosts::shell_services::{self, ShellVerb};

use crate::menu_bar::menu_bar;
use crate::model::AppModel;
use crate::theme;
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

    // The theme wrapper sits outside the menu bar and the body, so the Windows
    // immersive title bar and its menu items are descendants of the same theme
    // provider. Attaching `.theme(..)` inside the root view would leave that
    // sibling title bar on the framework's default dark fallback.
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
