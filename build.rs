fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=assets/logo_box.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("assets/logo_box.ico")
        .compile()?;
    Ok(())
}
