mod app;
mod browser;
mod compress_service;
mod extract_service;
mod menu_bar;
mod model;
mod places;
mod theme;
mod views;

cove_ui::application!(launch = app::application);
