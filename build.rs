fn main() -> Result<(), Box<dyn std::error::Error>> {
    slint_build::compile("ui/main.slint")?;
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/logo_box.ico")
        .compile()?;
    Ok(())
}
