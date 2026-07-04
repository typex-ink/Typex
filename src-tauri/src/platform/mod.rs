//! 平台层：OS 探测、权限检测、平台专用胶水（07 §4）。不依赖任何 service。
pub mod permissions;

#[cfg(target_os = "macos")]
pub mod macos;
