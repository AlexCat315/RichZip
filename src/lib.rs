mod app;
mod browser;
mod compress_service;
mod extract_service;
mod menu_bar;
mod model;
mod places;
mod theme;
mod views;
mod widget;

/// Declares the application for the current launch mode. `mugenzip --widget`
/// starts the floating ball; every other invocation starts the browser.
fn application() -> cove_ui::ApplicationLaunch {
    if std::env::args_os().skip(1).any(|arg| arg == "--widget") {
        widget::widget_application()
    } else {
        app::application()
    }
}

pub fn run() -> Result<(), String> {
    cove_ui::ApplicationLaunch::launch(application)
}
