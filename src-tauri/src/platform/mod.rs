//! 平台层：OS 探测、权限检测、平台专用胶水（06 §4）。不依赖任何 service。
pub mod focus;
pub mod permissions;
pub mod shell;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PlatformCapabilityStatus {
    pub key: String,
    pub available: bool,
    pub detail: String,
}

impl PlatformCapabilityStatus {
    pub fn available(key: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            available: true,
            detail: detail.into(),
        }
    }

    pub fn unavailable(key: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            available: false,
            detail: detail.into(),
        }
    }
}

pub fn capability_diagnostics() -> Vec<PlatformCapabilityStatus> {
    #[cfg(target_os = "windows")]
    {
        windows::capability_diagnostics()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Vec::new()
    }
}
