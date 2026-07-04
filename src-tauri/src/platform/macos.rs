//! macOS 专用胶水（NSPanel 处理在 app/windows.rs 使用；此处放纯平台函数）。

/// 请求辅助功能权限（弹系统引导对话框）。
pub fn prompt_accessibility() {
    macos_accessibility_client::accessibility::application_is_trusted_with_prompt();
}
