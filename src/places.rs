//! Where the app is allowed to look: the host's own folders, asked for the way
//! the host asks for them.
//!
//! Every platform spells these differently and none of the spellings is stable:
//!
//! - macOS keeps `Desktop` / `Documents` / `Downloads` under the home folder.
//! - Windows resolves them through `SHGetKnownFolderPath`, because `Desktop` and
//!   `Documents` are routinely redirected into OneDrive and the real path has
//!   nothing to do with `%USERPROFILE%`.
//! - Linux follows the XDG user-dirs spec: `user-dirs.dirs` is the authority,
//!   not `~/Downloads`, and on a non-English desktop the folders are named
//!   `桌面` / `下载` / `文档`.
//!
//! Guessing names under the home folder works on exactly one of those. So the
//! role is asked for, and the path is whatever the host answers.

use std::path::PathBuf;

/// Which well-known folder a row stands for. Callers branch on the role, never
/// on the label, so a renamed or localized folder cannot change behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlaceRole {
    Desktop,
    Documents,
    Downloads,
    Home,
}

impl PlaceRole {
    /// The row title. One Chinese word per role, so the sidebar reads the same
    /// regardless of what the folder is called on disk.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Desktop => "桌面",
            Self::Documents => "文档",
            Self::Downloads => "下载",
            Self::Home => "个人文件夹",
        }
    }

    /// Sidebar order: the places a person checks first, then the home folder.
    const fn rank(self) -> u8 {
        match self {
            Self::Desktop => 0,
            Self::Documents => 1,
            Self::Downloads => 2,
            Self::Home => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KnownPlace {
    pub role: PlaceRole,
    pub label: &'static str,
    pub path: PathBuf,
}

impl KnownPlace {
    fn new(role: PlaceRole, path: PathBuf) -> Self {
        Self {
            role,
            label: role.label(),
            path,
        }
    }
}

/// The user's home folder, spelled the way this platform spells it. `HOME` is
/// not set on Windows, and `USERPROFILE` is not set on Unix.
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            return Some(PathBuf::from(profile));
        }
        let drive = std::env::var_os("HOMEDRIVE")?;
        let path = std::env::var_os("HOMEPATH")?;
        let mut home = PathBuf::from(drive);
        home.push(path);
        return Some(home);
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// The folders the sidebar offers. Empty only when the host has no answer at
/// all, which a launch cannot fix by guessing.
pub fn known_places() -> Vec<KnownPlace> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    let mut places: Vec<KnownPlace> = Vec::new();

    for (role, path) in host_places(&home) {
        // A redirected folder can point at a missing path (a disconnected share,
        // an unmounted volume); offering it would give the user a row that
        // cannot open.
        if !path.is_dir() {
            continue;
        }
        // XDG marks a folder "disabled" by pointing it at the home directory
        // itself. A row that duplicates the home folder is noise.
        if path == home {
            continue;
        }
        if places.iter().any(|place| place.path == path) {
            continue;
        }
        places.push(KnownPlace::new(role, path));
    }

    places.sort_by_key(|place| place.role.rank());

    // The home folder is always offered: it is the one place that exists on
    // every install, and it is where a user who never changed anything actually
    // keeps their archives.
    if home.is_dir() && !places.iter().any(|place| place.path == home) {
        places.push(KnownPlace::new(PlaceRole::Home, home));
    }
    places
}

#[cfg(target_os = "macos")]
fn host_places(home: &std::path::Path) -> Vec<(PlaceRole, PathBuf)> {
    vec![
        (PlaceRole::Desktop, home.join("Desktop")),
        (PlaceRole::Documents, home.join("Documents")),
        (PlaceRole::Downloads, home.join("Downloads")),
    ]
}

#[cfg(target_os = "windows")]
fn host_places(home: &std::path::Path) -> Vec<(PlaceRole, PathBuf)> {
    use known_folder::{DESKTOP, DOCUMENTS, DOWNLOADS};

    // The known-folder API is the only thing that knows about OneDrive redirection
    // and localized folder names. When it fails (it does not, on a normal login),
    // the plain profile layout is the fallback rather than an empty sidebar.
    vec![
        (
            PlaceRole::Desktop,
            known_folder::path(DESKTOP).unwrap_or_else(|| home.join("Desktop")),
        ),
        (
            PlaceRole::Documents,
            known_folder::path(DOCUMENTS).unwrap_or_else(|| home.join("Documents")),
        ),
        (
            PlaceRole::Downloads,
            known_folder::path(DOWNLOADS).unwrap_or_else(|| home.join("Downloads")),
        ),
    ]
}

#[cfg(target_os = "linux")]
fn host_places(home: &std::path::Path) -> Vec<(PlaceRole, PathBuf)> {
    vec![
        (
            PlaceRole::Desktop,
            xdg_dir("XDG_DESKTOP_DIR", "Desktop", home),
        ),
        (
            PlaceRole::Documents,
            xdg_dir("XDG_DOCUMENTS_DIR", "Documents", home),
        ),
        (
            PlaceRole::Downloads,
            xdg_dir("XDG_DOWNLOAD_DIR", "Downloads", home),
        ),
    ]
}

/// Resolves one XDG user dir. The environment wins over the config file, the
/// config file wins over the English default — that is the order the spec
/// defines, and it is what makes a `~/下载` desktop work.
#[cfg(target_os = "linux")]
fn xdg_dir(variable: &str, fallback: &str, home: &std::path::Path) -> PathBuf {
    if let Some(value) = std::env::var_os(variable)
        && !value.is_empty()
    {
        return expand_home(&value.to_string_lossy(), home);
    }
    if let Some(path) = user_dirs_entry(variable, home) {
        return path;
    }
    home.join(fallback)
}

/// Reads `XDG_<NAME>_DIR` out of `user-dirs.dirs`.
#[cfg(target_os = "linux")]
fn user_dirs_entry(variable: &str, home: &std::path::Path) -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let text = std::fs::read_to_string(config.join("user-dirs.dirs")).ok()?;
    parse_user_dirs(&text, variable, home)
}

/// The file is a list of `NAME="value"` pairs; `$HOME` is the only expansion it
/// is allowed to use. Parsed without touching the filesystem so the spec rules
/// are testable on every host.
#[cfg(any(target_os = "linux", test))]
fn parse_user_dirs(text: &str, variable: &str, home: &std::path::Path) -> Option<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            if name.trim() != variable {
                return None;
            }
            Some(expand_home(value.trim().trim_matches('"'), home))
        })
}

/// `$HOME/Desktop` → the real path. Anything else is taken as written.
#[cfg(any(target_os = "linux", test))]
fn expand_home(value: &str, home: &std::path::Path) -> PathBuf {
    match value.strip_prefix("$HOME") {
        Some(rest) => {
            let mut path = home.to_path_buf();
            path.push(rest.trim_start_matches('/'));
            path
        }
        None => PathBuf::from(value),
    }
}

/// The Windows known-folder API, declared by hand: the shell asks for three
/// GUIDs and frees the string with the allocator that produced it.
#[cfg(target_os = "windows")]
mod known_folder {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    pub const DESKTOP: Guid = Guid {
        data1: 0xB4BF_CC3A,
        data2: 0xDB2C,
        data3: 0x424C,
        data4: [0xB0, 0x29, 0x7F, 0xE9, 0x9A, 0x87, 0xC6, 0x41],
    };
    pub const DOCUMENTS: Guid = Guid {
        data1: 0xFDD3_9AD0,
        data2: 0x238F,
        data3: 0x46AF,
        data4: [0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7],
    };
    pub const DOWNLOADS: Guid = Guid {
        data1: 0x374D_E290,
        data2: 0x123F,
        data3: 0x4565,
        data4: [0x91, 0x64, 0x39, 0xC4, 0x92, 0x5E, 0x46, 0x7B],
    };

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHGetKnownFolderPath(
            id: *const Guid,
            flags: u32,
            token: *mut core::ffi::c_void,
            path: *mut *mut u16,
        ) -> i32;
    }

    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(pointer: *mut core::ffi::c_void);
    }

    pub fn path(id: Guid) -> Option<PathBuf> {
        let mut wide: *mut u16 = std::ptr::null_mut();
        // SAFETY: the GUID is a valid known-folder id, the token is the current
        // user, and the out pointer is written by the API before it returns.
        let status = unsafe { SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut wide) };
        if status < 0 || wide.is_null() {
            if !wide.is_null() {
                // SAFETY: a string handed out by SHGetKnownFolderPath is freed
                // with CoTaskMemFree.
                unsafe { CoTaskMemFree(wide.cast()) };
            }
            return None;
        }
        let mut length = 0usize;
        // SAFETY: the API returns a NUL-terminated buffer.
        unsafe {
            while *wide.add(length) != 0 {
                length += 1;
            }
        }
        // SAFETY: `length` counts the code units before that NUL.
        let units = unsafe { std::slice::from_raw_parts(wide, length) }.to_vec();
        // SAFETY: ownership of the buffer returns to us once it is copied.
        unsafe { CoTaskMemFree(wide.cast()) };
        Some(PathBuf::from(OsString::from_wide(&units)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Chinese desktop points `XDG_DOWNLOAD_DIR` at `~/下载`; the old code
    /// looked at `~/Downloads` and found nothing.
    #[test]
    fn localized_user_dirs_are_read() {
        let home = std::path::Path::new("/Users/someone");
        let config = "# comment\nXDG_DESKTOP_DIR=\"$HOME/桌面\"\nXDG_DOWNLOAD_DIR=\"$HOME/下载\"\n";
        assert_eq!(
            parse_user_dirs(config, "XDG_DOWNLOAD_DIR", home),
            Some(home.join("下载"))
        );
        assert_eq!(
            parse_user_dirs(config, "XDG_DESKTOP_DIR", home),
            Some(home.join("桌面"))
        );
        // An absolute value is taken as written, and a missing key is absent
        // rather than guessed.
        let absolute = "XDG_DOCUMENTS_DIR=\"/mnt/data/docs\"\n";
        assert_eq!(
            parse_user_dirs(absolute, "XDG_DOCUMENTS_DIR", home),
            Some(std::path::PathBuf::from("/mnt/data/docs"))
        );
        assert_eq!(parse_user_dirs(absolute, "XDG_DOWNLOAD_DIR", home), None);
    }

    #[test]
    fn every_role_has_its_own_label() {
        let labels = [
            PlaceRole::Desktop.label(),
            PlaceRole::Documents.label(),
            PlaceRole::Downloads.label(),
            PlaceRole::Home.label(),
        ];
        for (index, label) in labels.iter().enumerate() {
            assert!(!label.is_empty());
            assert!(
                labels[index + 1..].iter().all(|other| other != label),
                "role labels must stay distinct: {labels:?}"
            );
        }
    }

    #[test]
    fn home_is_always_offered_last() {
        let places = known_places();
        if home_dir().is_none() {
            assert!(places.is_empty());
            return;
        }
        assert!(
            places.iter().any(|place| place.role == PlaceRole::Home),
            "the home folder is the one place every install has"
        );
        assert_eq!(places.last().map(|place| place.role), Some(PlaceRole::Home));
        assert!(places.iter().all(|place| place.path.is_dir()));
    }

    /// Holds on the machine running the tests, not on the other ones: the point
    /// is that each host resolves its own folders instead of a shared guess.
    #[test]
    fn resolved_places_are_absolute_and_unique() {
        let places = known_places();
        for place in &places {
            assert!(place.path.is_absolute(), "{:?} must be absolute", place.path);
        }
        for (index, place) in places.iter().enumerate() {
            assert!(
                places[index + 1..].iter().all(|other| other.path != place.path),
                "a place must not be listed twice"
            );
        }
    }
}
