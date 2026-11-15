mod browser;
mod model;
mod theme;
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
            app.window.width = 1180;
            app.window.height = 760;
            app.window.min_width = 940;
            app.window.min_height = 620;
        })
        .menu_bar(menu_bar(model))
        .diagnostics(cove_ui::hosts::desktop::diagnostics::devtools::factory())
}

/// On Windows this mounts Cove's immersive title bar (custom caption buttons,
/// drag region, synthetic resize border); on macOS it installs the AppKit menu.
fn menu_bar(model: Rc<model::AppModel>) -> NativeMenuBar {
    let open = model.clone();
    let extract = model.clone();
    let choose = model.clone();
    let select_all = model.clone();
    let cancel = model.clone();
    let up = model.clone();

    let file = NativeMenu::named("文件")
        .item(
            NativeMenuAction::new("打开压缩包…", move || open.open_dialog())
                .with_shortcut("o", NativeMenuKeyModifiers::primary()),
        )
        .item(
            NativeMenuAction::new("解压全部", move || extract.extract())
                .with_shortcut("e", NativeMenuKeyModifiers::primary()),
        )
        .item(
            NativeMenuAction::new("解压到…", move || choose.choose_destination())
                .with_shortcut(
                    "e",
                    NativeMenuKeyModifiers::primary().union(NativeMenuKeyModifiers::SHIFT),
                ),
        );
    let edit = NativeMenu::named("编辑").item(
        NativeMenuAction::new("全选", move || select_all.select_all())
            .with_shortcut("a", NativeMenuKeyModifiers::primary()),
    );
    let archive = NativeMenu::named("归档")
        .item(NativeMenuAction::new("取消解压", move || cancel.cancel_extract()))
        .item(
            NativeMenuAction::new("上一级", move || up.navigate_up())
                .with_shortcut("[", NativeMenuKeyModifiers::primary()),
        );
    NativeMenuBar::new("MugenZip", vec![], vec![file, edit, archive])
}