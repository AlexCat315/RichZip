mod browser;
mod model;
mod view;

use std::rc::Rc;

use cove_ui::ApplicationLaunch;

pub fn application() -> ApplicationLaunch {
    let root = view::RootView {
        model: Rc::new(model::AppModel::new()),
    };
    ApplicationLaunch::new(root)
        .configure(|app| {
            app.window.title = "MugenZip".into();
            app.window.width = 1100;
            app.window.height = 720;
            app.window.min_width = 880;
            app.window.min_height = 560;
        })
        .diagnostics(cove_ui::hosts::desktop::diagnostics::devtools::factory())
}