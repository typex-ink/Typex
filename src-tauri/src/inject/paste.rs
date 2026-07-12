//! 剪贴板粘贴注入（全平台默认，06 §7.5）：
//! 保存剪贴板 → 写入文本 → 模拟 Cmd/Ctrl+V → 延迟 → 恢复剪贴板。
//! Windows 保存常见 HGLOBAL 格式；其他平台的恢复受 arboard 能力限制。

use super::{InjectionLatch, InjectionOutcome, Injector};
use crate::error::Result;
#[cfg(not(target_os = "windows"))]
use crate::error::{ErrorCode, TypexError};
#[cfg(not(target_os = "windows"))]
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
        #[cfg(target_os = "windows")]
        {
            super::windows::paste_text(text, self.paste_delay_ms)
        }

        #[cfg(not(target_os = "windows"))]
        {
            let mut clipboard = arboard::Clipboard::new().map_err(|e| {
                TypexError::new(ErrorCode::Internal, format!("打开剪贴板失败: {e}"))
            })?;

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
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
    }

    fn inject_targeted(
        &self,
        text: &str,
        target: Option<&crate::platform::focus::FocusTarget>,
    ) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            super::windows::paste_text_to(text, self.paste_delay_ms, target)
        }
        #[cfg(not(target_os = "windows"))]
        {
            if target.is_some_and(|target| !target.is_current()) {
                return Err(crate::error::TypexError::new(
                    crate::error::ErrorCode::NoFocus,
                    "foreground target changed before injection",
                ));
            }
            self.inject(text)
        }
    }

    fn inject_targeted_cancellable(
        &self,
        text: &str,
        target: Option<&crate::platform::focus::FocusTarget>,
        latch: &InjectionLatch,
    ) -> Result<InjectionOutcome> {
        #[cfg(target_os = "windows")]
        {
            super::windows::paste_text_to_cancellable(text, self.paste_delay_ms, target, latch)
        }
        #[cfg(not(target_os = "windows"))]
        {
            if target.is_some_and(|target| !target.is_current()) {
                return Err(TypexError::new(
                    ErrorCode::NoFocus,
                    "foreground target changed before injection",
                ));
            }
            let mut clipboard = arboard::Clipboard::new().map_err(|error| {
                TypexError::new(ErrorCode::Internal, format!("打开剪贴板失败: {error}"))
            })?;
            let saved = clipboard.get_text().ok();
            clipboard.set_text(text).map_err(|error| {
                TypexError::new(ErrorCode::Internal, format!("写剪贴板失败: {error}"))
            })?;
            std::thread::sleep(Duration::from_millis(self.paste_delay_ms.max(10)));

            let target_is_current = target.is_none_or(|target| target.is_current());
            if !target_is_current || !latch.commit() {
                restore_text_clipboard(&mut clipboard, saved);
                if !target_is_current {
                    return Err(TypexError::new(
                        ErrorCode::NoFocus,
                        "foreground target changed before injection",
                    ));
                }
                return Ok(InjectionOutcome::Cancelled);
            }

            let result = post_paste_shortcut();
            if result.is_ok() {
                std::thread::sleep(Duration::from_millis(200));
            }
            restore_text_clipboard(&mut clipboard, saved);
            result.map(|()| InjectionOutcome::Injected)
        }
    }

    fn name(&self) -> &'static str {
        "paste"
    }
}

#[cfg(not(target_os = "windows"))]
fn restore_text_clipboard(clipboard: &mut arboard::Clipboard, saved: Option<String>) {
    if let Some(previous) = saved {
        let _ = clipboard.set_text(previous);
    } else {
        let _ = clipboard.clear();
    }
}

#[cfg(not(target_os = "windows"))]
fn post_paste_shortcut() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        crate::platform::macos::post_command_shortcut(crate::platform::macos::KEY_CODE_V)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default()).map_err(|error| {
            TypexError::new(
                ErrorCode::PermissionMissing,
                format!("enigo 初始化失败（缺辅助功能权限？）: {error}"),
            )
        })?;
        enigo
            .key(Key::Control, Direction::Press)
            .and_then(|_| enigo.key(Key::Unicode('v'), Direction::Click))
            .and_then(|_| enigo.key(Key::Control, Direction::Release))
            .map_err(|error| {
                TypexError::new(
                    ErrorCode::PermissionMissing,
                    format!("模拟按键失败: {error}"),
                )
            })
    }
}
