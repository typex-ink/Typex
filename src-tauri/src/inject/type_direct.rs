//! 逐字输入注入（06 §7.5 备选 type_direct）：enigo text() 模拟键入。
//!
//! 适用：不接受粘贴的场景（部分终端/远程桌面/密码框式输入）。
//! 妥协：长文本慢（逐字符事件）；依赖辅助功能权限（与 paste 的模拟按键相同）。

use super::Injector;
use crate::error::Result;
#[cfg(not(target_os = "windows"))]
use crate::error::{ErrorCode, TypexError};

pub struct TypeDirectInjector;

impl Injector for TypeDirectInjector {
    fn inject(&self, text: &str) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            super::windows::send_unicode(text)
        }
        #[cfg(not(target_os = "windows"))]
        {
            use enigo::{Enigo, Keyboard, Settings};
            let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
                TypexError::new(
                    ErrorCode::PermissionMissing,
                    format!("enigo 初始化失败（缺辅助功能权限？）: {e}"),
                )
            })?;
            enigo
                .text(text)
                .map_err(|e| TypexError::new(ErrorCode::Internal, format!("逐字输入失败: {e}")))
        }
    }

    fn inject_targeted(
        &self,
        text: &str,
        target: Option<&crate::platform::focus::FocusTarget>,
    ) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            super::windows::send_unicode_to(text, target)
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

    fn name(&self) -> &'static str {
        "type_direct"
    }
}
