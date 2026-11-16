//! Hash verification from the file menu.
//!
//! The checksum is computed by the platform's own tool (`certutil` on Windows,
//! `shasum`/`sha*sum` on Unix); CRC32 is small enough to implement here and
//! avoids depending on a command that may be missing. The result is written
//! beside the file and opened in the default text viewer, so a user who picked
//! "SHA-256" gets an answer rather than a silent side effect.

use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    Crc32,
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    pub fn key(self) -> &'static str {
        match self {
            Self::Crc32 => "crc32",
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Sha512 => "sha512",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Crc32 => "CRC32",
            Self::Md5 => "MD5",
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha512 => "SHA-512",
        }
    }
}

pub fn write_report(path: &Path, algorithm: Algorithm) -> Result<PathBuf, String> {
    let digest = digest(path, algorithm)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let report = parent.join(format!("{name}.{}.txt", algorithm.key()));
    let body = format!(
        "MugenZip 校验\n\
         \n\
         文件：{}\n\
         算法：{}\n\
         结果：{}\n\
         大小：{} 字节\n",
        path.display(),
        algorithm.title(),
        digest,
        std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
    );
    std::fs::write(&report, body).map_err(|error| error.to_string())?;
    open_path(&report);
    Ok(report)
}

fn digest(path: &Path, algorithm: Algorithm) -> Result<String, String> {
    if algorithm == Algorithm::Crc32 {
        return crc32(path).map_err(|error| error.to_string());
    }
    platform_digest(path, algorithm)
}

#[cfg(windows)]
fn platform_digest(path: &Path, algorithm: Algorithm) -> Result<String, String> {
    let name = match algorithm {
        Algorithm::Md5 => "MD5",
        Algorithm::Sha1 => "SHA1",
        Algorithm::Sha256 => "SHA256",
        Algorithm::Sha512 => "SHA512",
        Algorithm::Crc32 => unreachable!(),
    };
    let output = std::process::Command::new(certutil_path())
        .args(["-hashfile", &path.to_string_lossy(), name])
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let expected = match algorithm {
        Algorithm::Md5 => 32,
        Algorithm::Sha1 => 40,
        Algorithm::Sha256 => 64,
        Algorithm::Sha512 => 128,
        Algorithm::Crc32 => 8,
    };
    text.lines()
        .map(str::trim)
        .find(|line| line.len() == expected && line.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|line| line.to_ascii_lowercase())
        .ok_or_else(|| "certutil 没有返回可识别的摘要".to_owned())
}

#[cfg(target_os = "macos")]
fn platform_digest(path: &Path, algorithm: Algorithm) -> Result<String, String> {
    let (program, args) = match algorithm {
        Algorithm::Md5 => ("md5", vec!["-q".to_owned(), path.to_string_lossy().into_owned()]),
        Algorithm::Sha1 => ("shasum", vec!["-a".to_owned(), "1".to_owned(), path.to_string_lossy().into_owned()]),
        Algorithm::Sha256 => ("shasum", vec!["-a".to_owned(), "256".to_owned(), path.to_string_lossy().into_owned()]),
        Algorithm::Sha512 => ("shasum", vec!["-a".to_owned(), "512".to_owned(), path.to_string_lossy().into_owned()]),
        Algorithm::Crc32 => unreachable!(),
    };
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    first_hex(&output)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_digest(path: &Path, algorithm: Algorithm) -> Result<String, String> {
    let program = match algorithm {
        Algorithm::Md5 => "md5sum",
        Algorithm::Sha1 => "sha1sum",
        Algorithm::Sha256 => "sha256sum",
        Algorithm::Sha512 => "sha512sum",
        Algorithm::Crc32 => unreachable!(),
    };
    let output = std::process::Command::new(program)
        .arg(path)
        .output()
        .map_err(|error| error.to_string())?;
    first_hex(&output)
}

#[cfg(unix)]
fn first_hex(output: &std::process::Output) -> Result<String, String> {
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(|token| token.to_ascii_lowercase())
        .ok_or_else(|| "哈希工具没有返回结果".to_owned())
}

fn crc32(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = vec![0u8; 1 << 20];
    let mut crc: u32 = 0xFFFF_FFFF;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = 0u32.wrapping_sub(crc & 1);
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
    }
    Ok(format!("{:08x}", !crc))
}

#[cfg(windows)]
fn certutil_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("certutil.exe"))
        .filter(|path| path.exists())
        .unwrap_or_else(|| PathBuf::from("certutil.exe"))
}

fn open_path(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let rundll32 = std::env::var_os("SystemRoot")
            .map(|root| PathBuf::from(root).join("System32").join("rundll32.exe"))
            .filter(|path| path.exists())
            .unwrap_or_else(|| PathBuf::from("rundll32.exe"));
        let _ = std::process::Command::new(rundll32)
            .args(["url.dll,FileProtocolHandler", &path.to_string_lossy()])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}
