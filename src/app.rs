use std::rc::Rc;

use cove_ui::ApplicationLaunch;

use crate::menu_bar::menu_bar;
use crate::model::AppModel;
use crate::views::RootView;

pub fn application() -> ApplicationLaunch {
    let model = Rc::new(AppModel::new());
    let root = RootView {
        model: model.clone(),
    };
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
