mod app;
mod browser;
mod compress_service;
mod extract_service;
mod menu_bar;
mod model;
mod places;
mod preview;
mod theme;
mod views;
#[cfg(target_os = "macos")]
mod widget;

/// Declares the application for the current launch mode. `mugenzip --widget`
/// starts the floating ball; every other invocation starts the browser.
fn application() -> cove_ui::ApplicationLaunch {
    #[cfg(target_os = "macos")]
    if std::env::args_os().skip(1).any(|arg| arg == "--widget") {
        return widget::widget_application();
    }
    app::application()
}

pub fn run() -> Result<(), String> {
    cove_ui::ApplicationLaunch::launch(application)
}
