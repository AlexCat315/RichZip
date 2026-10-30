#![windows_subsystem = "windows"]

fn main() -> Result<(), slint::PlatformError> {
    mugenzip::app::run()
}
