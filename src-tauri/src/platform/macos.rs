//! macOS 专用胶水（NSPanel 处理在 app/windows.rs 使用；此处放纯平台函数）。

use crate::error::{ErrorCode, Result, TypexError};

pub const KEY_CODE_C: u16 = 0x08;
pub const KEY_CODE_V: u16 = 0x09;
const KEY_CODE_COMMAND: u16 = 0x37;
const CG_HID_EVENT_TAP: u32 = 0;
const CG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;

/// 请求辅助功能权限（弹系统引导对话框）。
pub fn prompt_accessibility() {
    macos_accessibility_client::accessibility::application_is_trusted_with_prompt();
}

/// 发送 Command + 单个虚拟键码。
///
/// 这里刻意不用 enigo 的 `Key::Unicode`：macOS 26 起，enigo/rdev 查询输入法布局时会
/// 触发 HIToolbox 的主队列断言，后台线程里调用会 SIGTRAP。固定快捷键只需要虚拟键码。
pub fn post_command_shortcut(key_code: u16) -> Result<()> {
    use std::ffi::c_void;
    use std::ptr;

    type CGEventRef = *mut c_void;
    type CGEventSourceRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: u16,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
        fn CGEventPost(tap: u32, event: CGEventRef);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    unsafe fn make_event(
        source: CGEventSourceRef,
        key_code: u16,
        key_down: bool,
        flags: u64,
    ) -> Result<CGEventRef> {
        let event = unsafe { CGEventCreateKeyboardEvent(source, key_code, key_down) };
        if event.is_null() {
            return Err(TypexError::new(
                ErrorCode::PermissionMissing,
                "创建 CGEvent 失败（缺辅助功能权限？）",
            ));
        }
        unsafe { CGEventSetFlags(event, flags) };
        Ok(event)
    }

    let source = ptr::null();
    let sequence = [
        (KEY_CODE_COMMAND, true, CG_EVENT_FLAG_MASK_COMMAND),
        (key_code, true, CG_EVENT_FLAG_MASK_COMMAND),
        (key_code, false, CG_EVENT_FLAG_MASK_COMMAND),
        (KEY_CODE_COMMAND, false, 0),
    ];
    let mut events = Vec::with_capacity(sequence.len());
    for (key_code, key_down, flags) in sequence {
        match unsafe { make_event(source, key_code, key_down, flags) } {
            Ok(event) => events.push(event),
            Err(e) => {
                for event in events {
                    unsafe { CFRelease(event.cast_const()) };
                }
                return Err(e);
            }
        }
    }

    for event in &events {
        unsafe { CGEventPost(CG_HID_EVENT_TAP, *event) };
    }
    for event in events {
        unsafe { CFRelease(event.cast_const()) };
    }
    Ok(())
}
