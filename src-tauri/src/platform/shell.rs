//! Open local paths and system settings with the platform shell.

use std::path::Path;

pub fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        super::windows::shell_open_path(path)
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|_| "shell_open_failed".into())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|_| "shell_open_failed".into())
    }
}

pub fn open_uri(uri: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        super::windows::shell_open(uri.as_ref())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(uri)
            .spawn()
            .map(|_| ())
            .map_err(|_| "shell_open_failed".into())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(uri)
            .spawn()
            .map(|_| ())
            .map_err(|_| "shell_open_failed".into())
    }
}
