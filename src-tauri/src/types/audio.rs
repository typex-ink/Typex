//! Audio IPC types.

use serde::{Deserialize, Serialize};

/// Stable device identity plus the user-facing label shown in settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AudioInputDevice {
    pub id: String,
    pub label: String,
}
