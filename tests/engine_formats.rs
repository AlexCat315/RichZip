//! The engine crate keeps its public surface stable across the app boundary.

#[test]
fn formats_are_detected_from_the_extension() {
    use std::path::Path;

    assert!(mugenzip_core::detect_format(Path::new("book.cbz")).is_ok());
    assert!(mugenzip_core::detect_format(Path::new("package.tar.gz")).is_ok());
    assert!(mugenzip_core::detect_format(Path::new("notes.md")).is_err());
}

#[test]
#[ignore = "needs MUGENZIP_REAL_ARCHIVE pointing at a real package"]
fn real_archive_extracts_into_a_temp_folder() {
    use std::path::PathBuf;

    let Some(source) = std::env::var_os("MUGENZIP_REAL_ARCHIVE").map(PathBuf::from) else {
        return;
    };
    let destination =
        std::env::temp_dir().join(format!("mugenzip-real-{}", std::process::id()));
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut archive = mugenzip_core::open_archive(&source).expect("open");
    let report = archive
        .extract_entries(&destination, &|_| true, false, &mut |_| {}, &cancel)
        .expect("extract");
    assert!(report.completed > 0, "at least one file came out");
    assert!(
        std::fs::read_dir(&report.output_dir).unwrap().count() > 0,
        "the output folder is not empty"
    );
    std::fs::remove_dir_all(&destination).ok();
}
