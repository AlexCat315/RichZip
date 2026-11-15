use std::rc::Rc;

use cove_ui::{NativeMenu, NativeMenuAction, NativeMenuBar, NativeMenuKeyModifiers};

use crate::model::AppModel;

/// On Windows this mounts Cove's immersive title bar (custom caption buttons,
/// drag region, synthetic resize border); on macOS it installs the AppKit menu.
pub fn menu_bar(model: Rc<AppModel>) -> NativeMenuBar {
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