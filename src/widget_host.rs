//! Lifetime of the resident floating menu.
//!
//! The browser and the dot are two launch modes of one executable on
//! Windows/Linux (`mugenzip` and `mugenzip --widget`), and two bundles on
//! macOS. The browser owns the preference; this module turns that preference
//! into a process that survives the settings sheet, and records its pid so
//! both modes can tell "already running" from "start one".

use std::path::PathBuf;

use crate::settings;

pub const WIDGET_FLAG: &str = "--widget";

/// The browser executable. From the dot bundle on macOS this is the sibling
/// `MugenZip.app`; everywhere else it is the same binary.
pub fn main_executable() -> PathBuf {
    let Ok(executable) = std::env::current_exe() else {
        return PathBuf::from("mugenzip");
    };
    #[cfg(target_os = "macos")]
    {
        let path = executable.to_string_lossy();
        if let Some(stem) = path.find("MugenZipWidget.app") {
            let browser = format!(
                "{}MugenZip.app{}",
                &path[..stem],
                &path[stem + "MugenZipWidget.app".len()..]
            );
            if PathBuf::from(&browser).is_file() {
                return PathBuf::from(browser);
            }
        }
    }
    executable
}

/// The dot executable, wherever the platform keeps it.
pub fn widget_executable() -> PathBuf {
    let Ok(executable) = std::env::current_exe() else {
        return PathBuf::from("mugenzip");
    };
    #[cfg(target_os = "macos")]
    {
        let path = executable.to_string_lossy();
        if let Some(stem) = path.find("MugenZip.app") {
            let helper = format!(
                "{}MugenZipWidget.app{}",
                &path[..stem],
                &path[stem + "MugenZip.app".len()..]
            );
            if PathBuf::from(&helper).is_file() {
                return PathBuf::from(helper);
            }
        }
    }
    executable
}

fn pid_path() -> Option<PathBuf> {
    settings::config_dir().map(|dir| dir.join("widget.pid"))
}

fn main_pid_path() -> Option<PathBuf> {
    settings::config_dir().map(|dir| dir.join("main.pid"))
}

pub fn record_main_pid() {
    let Some(path) = main_pid_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, std::process::id().to_string());
}

/// PID of the browser instance, when a previous launch left one alive.
pub fn running_main_pid() -> Option<u32> {
    let path = main_pid_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let pid = text.trim().parse::<u32>().ok()?;
    if process_is_alive(pid) {
        Some(pid)
    } else {
        let _ = std::fs::remove_file(path);
        None
    }
}

pub fn record_widget_pid() {
    let Some(path) = pid_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, std::process::id().to_string());
}

pub fn clear_widget_pid() {
    if let Some(path) = pid_path() {
        let _ = std::fs::remove_file(path);
    }
}

pub fn running_widget_pid() -> Option<u32> {
    let path = pid_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let pid = text.trim().parse::<u32>().ok()?;
    if process_is_alive(pid) {
        Some(pid)
    } else {
        clear_widget_pid();
        None
    }
}

pub fn is_widget_running() -> bool {
    running_widget_pid().is_some()
}

/// Starts the dot unless one is already alive. Returns whether a process was
/// spawned by this call (an already-running dot is not an error).
pub fn start_widget() -> bool {
    if is_widget_running() {
        return false;
    }
    let executable = widget_executable();
    let mut command = std::process::Command::new(executable);
    command.arg(WIDGET_FLAG);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn().is_ok()
}

pub fn stop_widget() {
    let Some(pid) = running_widget_pid() else {
        return;
    };
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        // SIGTERM lets the helper remove its tray item and release its hotkey.
        unsafe {
            kill(pid as i32, 15);
        }
    }
    clear_widget_pid();
}

/// Applies the setting to the live process and the login entry.
pub fn apply(enabled: bool) {
    if enabled {
        start_widget();
    } else {
        stop_widget();
    }
    sync_autostart(enabled);
}

fn sync_autostart(enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        if enabled {
            let command = format!("\"{}\" {}", widget_executable().display(), WIDGET_FLAG);
            let _ = std::process::Command::new("reg.exe")
                .args([
                    "add",
                    key,
                    "/v",
                    "MugenZipWidget",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &command,
                    "/f",
                ])
                .status();
        } else {
            let _ = std::process::Command::new("reg.exe")
                .args(["delete", key, "/v", "MugenZipWidget", "/f"])
                .status();
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let Some(config) = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
            })
        else {
            return;
        };
        let path = config.join("autostart").join("mugenzip-widget.desktop");
        if enabled {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let entry = format!(
                "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=MugenZip 悬浮球\n\
                 Comment=常驻桌面的 MugenZip 快捷菜单\n\
                 Exec=\"{}\" {}\n\
                 Terminal=false\n\
                 X-GNOME-Autostart-enabled=true\n\
                 NoDisplay=true\n",
                widget_executable().display(),
                WIDGET_FLAG
            );
            let _ = std::fs::write(path, entry);
        } else {
            // A user-level file with Hidden=true overrides the system autostart
            // entry shipped by the package.
            let entry = "[Desktop Entry]\nType=Application\nName=MugenZip 悬浮球\nHidden=true\n";
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, entry);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join("com.mugenzip.widget.plist");
        if enabled {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let program = widget_executable();
            let body = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1\">\n<dict>\n\
                 <key>Label</key><string>com.mugenzip.widget</string>\n\
                 <key>ProgramArguments</key><array>\n\
                 <string>{}</string><string>{}</string>\n\
                 </array>\n\
                 <key>RunAtLoad</key><true/>\n\
                 </dict></plist>\n",
                xml_escape(&program.to_string_lossy()),
                WIDGET_FLAG
            );
            let _ = std::fs::write(path, body);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(target_os = "macos")]
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use std::ffi::c_void;
    type Handle = *mut c_void;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    unsafe { kill(pid as i32, 0) == 0 }
}

/// `true` when the helper can run on this desktop. Shape and global hotkeys
/// are optional; the dot still works as a small always-on-top window when a
/// compositor refuses the shaped surface.
pub fn supported() -> bool {
    true
}
