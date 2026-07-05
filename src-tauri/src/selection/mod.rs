//! 读取选中文本：trait SelectionReader + 平台降级链（07 §7.6）。
//!
//! macOS：AX API 主路径 → CGEvent Cmd+C + 剪贴板降级（读完恢复；
//! 对比复制前后剪贴板 + 超时防误触）。

use crate::error::{ErrorCode, Result, TypexError};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub trait SelectionReader: Send + Sync {
    /// 读取当前选中文本；None = 无选区。
    fn read(&self) -> Result<Option<String>>;

    /// 读取当前选中文本的屏幕 bounds；None = 无选区或目标应用不支持。
    fn read_bounds(&self) -> Result<Option<SelectionBounds>> {
        Ok(None)
    }
}

/// 平台默认实现。
pub fn platform_default() -> Box<dyn SelectionReader> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacSelectionReader)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(ClipboardFallbackReader)
    }
}

#[cfg(target_os = "macos")]
pub struct MacSelectionReader;

#[cfg(target_os = "macos")]
impl SelectionReader for MacSelectionReader {
    fn read(&self) -> Result<Option<String>> {
        // 主路径：AX kAXSelectedTextAttribute
        match ax_selected_text() {
            Ok(Some(text)) if !text.is_empty() => return Ok(Some(text)),
            // 元素明确报告选区为空 → 无选区，不做侵入式降级（Cmd+C 探测有副作用）
            Ok(Some(_)) => return Ok(None),
            Ok(None) => {} // 元素不支持选区属性 → 剪贴板降级
            Err(e) => tracing::debug!("AX 读选区失败，走剪贴板降级: {}", e.message),
        }
        // 降级：Cmd+C + 剪贴板
        ClipboardFallbackReader.read()
    }

    fn read_bounds(&self) -> Result<Option<SelectionBounds>> {
        ax_selected_text_bounds()
    }
}

/// AX API 读取焦点元素的选中文本（07 §7.6-1）。
/// `Ok(Some(text))` = 元素支持选区属性（text 可为空 = 明确无选区）；`Ok(None)` = 元素不支持。
#[cfg(target_os = "macos")]
fn ax_selected_text() -> Result<Option<String>> {
    use std::ffi::c_void;
    use std::ptr;

    #[repr(C)]
    struct __AXUIElement(c_void);
    type AXUIElementRef = *const __AXUIElement;
    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type AXError = i32;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const i8,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetCString(s: CFStringRef, buf: *mut i8, size: isize, encoding: u32) -> bool;
        fn CFStringGetLength(s: CFStringRef) -> isize;
        fn CFRelease(cf: CFTypeRef);
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
    }
    const UTF8: u32 = 0x0800_0100;

    unsafe {
        let cf_str = |s: &str| {
            let c = std::ffi::CString::new(s).unwrap();
            CFStringCreateWithCString(ptr::null(), c.as_ptr(), UTF8)
        };
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return Err(TypexError::new(ErrorCode::PermissionMissing, "AX 不可用"));
        }
        let attr_focused = cf_str("AXFocusedUIElement");
        let mut focused: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(system, attr_focused, &mut focused);
        CFRelease(attr_focused as CFTypeRef);
        CFRelease(system as CFTypeRef);
        if err != 0 || focused.is_null() {
            return Err(TypexError::new(
                ErrorCode::Internal,
                format!("无焦点元素 (AXError {err})"),
            ));
        }
        let attr_sel = cf_str("AXSelectedText");
        let mut sel: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(focused as AXUIElementRef, attr_sel, &mut sel);
        CFRelease(attr_sel as CFTypeRef);
        CFRelease(focused);
        if err != 0 || sel.is_null() {
            return Ok(None); // 元素不支持选区属性
        }
        if CFGetTypeID(sel) != CFStringGetTypeID() {
            CFRelease(sel);
            return Ok(None);
        }
        let len = CFStringGetLength(sel);
        let buf_size = len * 4 + 1;
        let mut buf = vec![0i8; buf_size as usize];
        let ok = CFStringGetCString(sel, buf.as_mut_ptr(), buf_size, UTF8);
        CFRelease(sel);
        if !ok {
            return Ok(None);
        }
        let text = std::ffi::CStr::from_ptr(buf.as_ptr())
            .to_string_lossy()
            .into_owned();
        Ok(Some(text)) // 空字符串 = 元素支持属性但当前无选区
    }
}

/// AX API 读取选中文本 bounds（屏幕点坐标；目标应用不支持时返回 None）。
#[cfg(target_os = "macos")]
fn ax_selected_text_bounds() -> Result<Option<SelectionBounds>> {
    use std::ffi::c_void;
    use std::ptr;

    #[repr(C)]
    struct __AXUIElement(c_void);
    type AXUIElementRef = *const __AXUIElement;
    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type AXValueRef = *const c_void;
    type AXError = i32;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CFRange {
        location: isize,
        length: isize,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CGRect {
        origin: CGPoint,
        size: CGSize,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
        fn AXUIElementCopyParameterizedAttributeValue(
            element: AXUIElementRef,
            parameterized_attribute: CFStringRef,
            parameter: CFTypeRef,
            result: *mut CFTypeRef,
        ) -> AXError;
        fn AXValueGetValue(value: AXValueRef, value_type: u32, value_ptr: *mut c_void) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const i8,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: CFTypeRef);
    }

    const UTF8: u32 = 0x0800_0100;
    const K_AX_VALUE_TYPE_CG_RECT: u32 = 3;
    const K_AX_VALUE_TYPE_CF_RANGE: u32 = 4;

    unsafe {
        let cf_str = |s: &str| {
            let c = std::ffi::CString::new(s).unwrap();
            CFStringCreateWithCString(ptr::null(), c.as_ptr(), UTF8)
        };

        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return Err(TypexError::new(ErrorCode::PermissionMissing, "AX 不可用"));
        }

        let attr_focused = cf_str("AXFocusedUIElement");
        let mut focused: CFTypeRef = ptr::null();
        let err = AXUIElementCopyAttributeValue(system, attr_focused, &mut focused);
        CFRelease(attr_focused as CFTypeRef);
        CFRelease(system as CFTypeRef);
        if err != 0 || focused.is_null() {
            return Err(TypexError::new(
                ErrorCode::Internal,
                format!("无焦点元素 (AXError {err})"),
            ));
        }

        let attr_range = cf_str("AXSelectedTextRange");
        let mut range_value: CFTypeRef = ptr::null();
        let err =
            AXUIElementCopyAttributeValue(focused as AXUIElementRef, attr_range, &mut range_value);
        CFRelease(attr_range as CFTypeRef);
        if err != 0 || range_value.is_null() {
            CFRelease(focused);
            return Ok(None);
        }

        let mut range = CFRange::default();
        let ok = AXValueGetValue(
            range_value as AXValueRef,
            K_AX_VALUE_TYPE_CF_RANGE,
            (&mut range as *mut CFRange).cast(),
        );
        if !ok || range.length <= 0 {
            CFRelease(range_value);
            CFRelease(focused);
            return Ok(None);
        }

        let attr_bounds = cf_str("AXBoundsForRange");
        let mut bounds_value: CFTypeRef = ptr::null();
        let err = AXUIElementCopyParameterizedAttributeValue(
            focused as AXUIElementRef,
            attr_bounds,
            range_value,
            &mut bounds_value,
        );
        CFRelease(attr_bounds as CFTypeRef);
        CFRelease(range_value);
        CFRelease(focused);
        if err != 0 || bounds_value.is_null() {
            return Ok(None);
        }

        let mut rect = CGRect::default();
        let ok = AXValueGetValue(
            bounds_value as AXValueRef,
            K_AX_VALUE_TYPE_CG_RECT,
            (&mut rect as *mut CGRect).cast(),
        );
        CFRelease(bounds_value);
        if !ok || rect.size.width <= 0.0 || rect.size.height <= 0.0 {
            return Ok(None);
        }

        Ok(Some(SelectionBounds {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        }))
    }
}

/// 剪贴板降级：保存 → 模拟 Cmd/Ctrl+C → 读取 → 恢复（07 §7.6）。
pub struct ClipboardFallbackReader;

impl SelectionReader for ClipboardFallbackReader {
    fn read(&self) -> Result<Option<String>> {
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("剪贴板不可用: {e}")))?;
        let saved = clipboard.get_text().ok();

        // 写入哨兵值：复制后内容不变 = 没有选中内容（防误读旧剪贴板）
        const SENTINEL: &str = "\u{200B}typex-selection-probe\u{200B}";
        clipboard
            .set_text(SENTINEL)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("写剪贴板失败: {e}")))?;

        #[cfg(target_os = "macos")]
        {
            crate::platform::macos::post_command_shortcut(crate::platform::macos::KEY_CODE_C)?;
        }
        #[cfg(not(target_os = "macos"))]
        {
            use enigo::{Direction, Enigo, Key, Keyboard, Settings};
            let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
                TypexError::new(ErrorCode::PermissionMissing, format!("enigo: {e}"))
            })?;
            enigo
                .key(Key::Control, Direction::Press)
                .and_then(|_| enigo.key(Key::Unicode('c'), Direction::Click))
                .and_then(|_| enigo.key(Key::Control, Direction::Release))
                .map_err(|e| {
                    TypexError::new(
                        ErrorCode::PermissionMissing,
                        format!("模拟 Ctrl+C 失败: {e}"),
                    )
                })?;
        }

        // 300ms 超时轮询（07 §7.6-4）
        let mut text = None;
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(30));
            if let Ok(t) = clipboard.get_text()
                && t != SENTINEL
            {
                text = Some(t);
                break;
            }
        }

        // 恢复原剪贴板
        match saved {
            Some(prev) => {
                let _ = clipboard.set_text(prev);
            }
            None => {
                let _ = clipboard.clear();
            }
        }
        Ok(text.filter(|t| !t.is_empty()))
    }
}
