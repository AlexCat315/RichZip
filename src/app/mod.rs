mod browser;
mod model;
mod view;

use std::rc::Rc;

use cove_ui::{
    ApplicationLaunch, NativeMenu, NativeMenuAction, NativeMenuBar, NativeMenuKeyModifiers,
};

pub fn application() -> ApplicationLaunch {
    let model = Rc::new(model::AppModel::new());
    let root = view::RootView {
        model: model.clone(),
    };
    ApplicationLaunch::new(root)
        .configure(|app| {
            app.window.title = "MugenZip".into();
            app.window.width = 1100;
            app.window.height = 720;
            app.window.min_width = 880;
            app.window.min_height = 560;
        })
        .menu_bar(menu_bar(model))
        .diagnostics(cove_ui::hosts::desktop::diagnostics::devtools::factory())
}

/// On Windows this mounts Cove's immersive title bar (custom caption buttons,
/// drag region, synthetic resize border); on macOS it installs the AppKit menu.
fn menu_bar(model: Rc<model::AppModel>) -> NativeMenuBar {
    let open = model.clone();
    let extract = model.clone();
    let up = model.clone();
    let file = NativeMenu::named("文件")
        .item(
            NativeMenuAction::new("打开压缩包…", move || open.open_dialog())
                .with_shortcut("o", NativeMenuKeyModifiers::primary()),
        )
        .item(
            NativeMenuAction::new("解压到…", move || extract.extract_dialog())
                .with_shortcut("e", NativeMenuKeyModifiers::primary()),
        );
    let go = NativeMenu::named("前往").item(
        NativeMenuAction::new("上一级", move || up.navigate_up())
            .with_shortcut("[", NativeMenuKeyModifiers::primary()),
    );
    NativeMenuBar::new("MugenZip", vec![], vec![file, go])
}