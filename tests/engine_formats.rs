//! The engine crate keeps its public surface stable across the app boundary.

#[test]
fn formats_are_detected_from_the_extension() {
    use std::path::Path;

    assert!(mugenzip_core::detect_format(Path::new("book.cbz")).is_ok());
    assert!(mugenzip_core::detect_format(Path::new("package.tar.gz")).is_ok());
    assert!(mugenzip_core::detect_format(Path::new("notes.md")).is_err());
}
