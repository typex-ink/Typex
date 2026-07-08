//! 剪贴板粘贴注入（全平台默认，06 §7.5）：
//! 保存剪贴板 → 写入文本 → 模拟 Cmd/Ctrl+V → 延迟 → 恢复剪贴板。
//! 已知妥协：恢复仅支持文本（arboard 能力边界，06 §7.2-8）。

use super::Injector;
use crate::error::{ErrorCode, Result, TypexError};
use std::time::Duration;

pub struct PasteInjector {
    paste_delay_ms: u64,
}

impl PasteInjector {
    pub fn new(paste_delay_ms: u64) -> Self {
        Self { paste_delay_ms }
    }
}

impl Injector for PasteInjector {
    fn inject(&self, text: &str) -> Result<()> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("打开剪贴板失败: {e}")))?;

        // 1. 保存原剪贴板（仅文本；无内容/非文本 = None）
        let saved = clipboard.get_text().ok();

        // 2. 写入待注入文本
        clipboard
            .set_text(text)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("写剪贴板失败: {e}")))?;

        // 写剪贴板到目标应用可读之间需要短暂延迟（平台坑 7.2-4）
        std::thread::sleep(Duration::from_millis(self.paste_delay_ms.max(10)));

        // 3. 模拟粘贴组合键
        #[cfg(target_os = "macos")]
        {
            crate::platform::macos::post_command_shortcut(crate::platform::macos::KEY_CODE_V)?;
        }
        #[cfg(not(target_os = "macos"))]
        {
            use enigo::{Direction, Enigo, Key, Keyboard, Settings};
            let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
                TypexError::new(
                    ErrorCode::PermissionMissing,
                    format!("enigo 初始化失败（缺辅助功能权限？）: {e}"),
                )
            })?;
            enigo
                .key(Key::Control, Direction::Press)
                .and_then(|_| enigo.key(Key::Unicode('v'), Direction::Click))
                .and_then(|_| enigo.key(Key::Control, Direction::Release))
                .map_err(|e| {
                    TypexError::new(ErrorCode::PermissionMissing, format!("模拟按键失败: {e}"))
                })?;
        }

        // 4. 等待目标应用读取后恢复剪贴板
        std::thread::sleep(Duration::from_millis(200));
        if let Some(prev) = saved {
            let _ = clipboard.set_text(prev); // 恢复失败不吞注入成功的结果
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "paste"
    }
}
