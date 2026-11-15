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
    let smart = model.clone();
    let here = model.clone();
    let named = model.clone();
    let extract = model.clone();
    let select_all = model.clone();
    let inspector = model.clone();
    let up = model.clone();
    let file = NativeMenu::named("文件")
        .item(
            NativeMenuAction::new("打开压缩包…", move || open.open_dialog())
                .with_shortcut("o", NativeMenuKeyModifiers::primary()),
        )
        .item(
            NativeMenuAction::new("智能解压", move || smart.extract_smart())
                .with_shortcut("e", NativeMenuKeyModifiers::primary()),
        )
        .item(NativeMenuAction::new("解压到当前文件夹", move || {
            here.extract_here()
        }))
        .item(NativeMenuAction::new("解压到名称文件夹", move || {
            named.extract_named()
        }))
        .item(
            NativeMenuAction::new("解压到…", move || extract.extract_dialog())
                .with_shortcut("e", NativeMenuKeyModifiers::primary().union(NativeMenuKeyModifiers::SHIFT)),
        );
    let edit = NativeMenu::named("编辑").item(
        NativeMenuAction::new("全选", move || select_all.select_all())
            .with_shortcut("a", NativeMenuKeyModifiers::primary()),
    );
    let view_menu = NativeMenu::named("显示").item(NativeMenuAction::new(
        "显示或隐藏简介",
        move || inspector.toggle_inspector(),
    ));
    let go = NativeMenu::named("前往").item(
        NativeMenuAction::new("上一级", move || up.navigate_up())
            .with_shortcut("[", NativeMenuKeyModifiers::primary()),
    );
    NativeMenuBar::new("MugenZip", vec![], vec![file, edit, view_menu, go])
}