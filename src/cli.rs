//! Context-menu command line.
//!
//! The file manager starts a short-lived process, not the browsing window. The
//! Windows registry and Linux `.desktop` actions all route to the same
//! `mugenzip --verb <id> <paths…>` shape, so the three platforms share one
//! implementation and one set of behaviours.

use std::path::PathBuf;

use mugenzip_core::{CompressionLevel, CreateFormat};

use crate::{compress_service, extract_service, hash_service, settings::Settings, widget_host};

pub fn try_run() -> Option<Result<(), String>> {
    let mut args = std::env::args_os().skip(1);
    let flag = args.next()?;
    if flag != "--verb" {
        return None;
    }
    let verb = args.next()?.to_string_lossy().into_owned();
    let paths: Vec<PathBuf> = args.map(PathBuf::from).collect();
    Some(run_verb(&verb, paths))
}

fn run_verb(verb: &str, paths: Vec<PathBuf>) -> Result<(), String> {
    if paths.is_empty() {
        return Err("没有收到要处理的文件".into());
    }
    match verb {
        "open" => {
            // A click on "用 MugenZip 打开" should open the browser, not extract.
            let executable = widget_host::main_executable();
            let mut command = std::process::Command::new(executable);
            command.arg(&paths[0]);
            command
                .spawn()
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        "extract-here" | "extract-folder" | "smart-extract" => {
            let placement = match verb {
                "extract-here" => extract_service::Placement::Here,
                "smart-extract" => extract_service::Placement::Smart,
                _ => extract_service::Placement::IntoFolder,
            };
            let mut last_output = None;
            for path in paths {
                last_output = extract_service::run_blocking(path, placement);
            }
            if let Some(output) = last_output {
                cove_ui::hosts::shell_services::reveal(&output);
            }
            Ok(())
        }
        "compress-here" | "compress-zip" | "compress-7z" => {
            let settings = Settings::load();
            let format = if verb == "compress-7z" {
                CreateFormat::SevenZ
            } else if verb == "compress-here" {
                settings.default_create_format
            } else {
                CreateFormat::Zip
            };
            let level = if verb == "compress-here" {
                settings.default_create_level
            } else {
                CompressionLevel::Balanced
            };
            if let Some(path) = compress_service::run_blocking(paths, format, level) {
                cove_ui::hosts::shell_services::reveal(&path);
            }
            Ok(())
        }
        "compress-ui" => {
            // Hand the selection to the browser's create sheet. It is the one
            // verb that wants a window, because the person is choosing a name,
            // format and destination.
            let executable = widget_host::main_executable();
            let mut command = std::process::Command::new(executable);
            command.arg("--create");
            command.args(&paths);
            command
                .spawn()
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        verb if verb.starts_with("hash-") => {
            let algorithm = match verb.trim_start_matches("hash-") {
                "crc32" => hash_service::Algorithm::Crc32,
                "md5" => hash_service::Algorithm::Md5,
                "sha1" => hash_service::Algorithm::Sha1,
                "sha256" => hash_service::Algorithm::Sha256,
                "sha512" => hash_service::Algorithm::Sha512,
                other => return Err(format!("未知哈希算法：{other}")),
            };
            for path in paths {
                hash_service::write_report(&path, algorithm)?;
            }
            Ok(())
        }
        other => Err(format!("未知右键动作：{other}")),
    }
}
