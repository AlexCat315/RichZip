mod app;
mod browser;
mod cli;
mod compress_service;
mod extract_service;
mod hash_service;
mod menu_bar;
mod model;
mod places;
mod preview;
mod theme;
mod settings;
mod views;
mod widget;
mod widget_host;

/// Declares the application for the current launch mode. `mugenzip --widget`
/// starts the floating ball; every other invocation starts the browser. The
/// widget is a launch mode of the same binary on every desktop platform, so the
/// Windows setup and the Linux package get the same two-entry experience as the
/// macOS app pair.
fn application() -> cove_ui::ApplicationLaunch {
    if std::env::args_os().skip(1).any(|arg| arg == "--widget") {
        return widget::widget_application();
    }
    app::application()
}

pub fn run() -> Result<(), String> {
    if let Some(result) = cli::try_run() {
        return result;
    }
    cove_ui::ApplicationLaunch::launch(application)
}
