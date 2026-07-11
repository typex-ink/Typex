//! Windows platform glue. Unsafe Win32 calls stay in this module.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_SUCCESS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, RECT, SetLastError,
    WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, RDW_FRAME,
    RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
};
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
    TOKEN_QUERY, TokenIntegrityLevel,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, DestroyIcon, GUITHREADINFO, GWL_EXSTYLE, GWL_STYLE, GetClassNameW,
    GetForegroundWindow, GetGUIThreadInfo, GetWindowLongPtrW, GetWindowThreadProcessId, HICON,
    HWND_TOPMOST, ICON_BIG, ICON_SMALL, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LR_SHARED,
    LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SW_SHOWNORMAL, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SendMessageW, SetWindowLongPtrW,
    SetWindowPos, WM_NCACTIVATE, WM_SETICON, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
};
use windows::core::{PCWSTR, PWSTR, w};

use super::PlatformCapabilityStatus;

const BASE_DPI: f64 = 96.0;
const PROCESS_IMAGE_BUFFER_U16: usize = 32_768;
const INK_ICON_ICO: &[u8] = include_bytes!("../../icons/icon-ink.ico");
static CUSTOM_APP_ICONS: OnceLock<Mutex<HashMap<isize, isize>>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalScreenPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalPixelSize {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PhysicalScreenRect {
    pub fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    pub fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }
}

impl From<RECT> for PhysicalScreenRect {
    fn from(value: RECT) -> Self {
        Self {
            left: value.left,
            top: value.top,
            right: value.right,
            bottom: value.bottom,
        }
    }
}

/// Screen-space logical DIPs. On Windows, `SelectionBounds` uses this unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalScreenRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Geometry for the monitor containing the target foreground HWND.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorWorkArea {
    pub monitor_px: PhysicalScreenRect,
    pub work_area_px: PhysicalScreenRect,
    pub scale_factor: f64,
}

impl MonitorWorkArea {
    fn valid_scale(self) -> f64 {
        if self.scale_factor.is_finite() && self.scale_factor > 0.0 {
            self.scale_factor
        } else {
            1.0
        }
    }
}

/// Convert Win32 physical screen pixels to Tauri logical screen DIPs.
///
/// The target monitor is explicit so mixed-DPI layouts and negative monitor origins do not use
/// the HUD window's (possibly different) scale factor.
pub fn physical_rect_to_logical_screen(
    rect: PhysicalScreenRect,
    monitor: MonitorWorkArea,
) -> LogicalScreenRect {
    let scale = monitor.valid_scale();
    let logical_origin_x = f64::from(monitor.monitor_px.left) / scale;
    let logical_origin_y = f64::from(monitor.monitor_px.top) / scale;
    LogicalScreenRect {
        x: logical_origin_x + (f64::from(rect.left) - f64::from(monitor.monitor_px.left)) / scale,
        y: logical_origin_y + (f64::from(rect.top) - f64::from(monitor.monitor_px.top)) / scale,
        width: f64::from(rect.width()) / scale,
        height: f64::from(rect.height()) / scale,
    }
}

/// Convert Tauri logical screen DIPs back to Win32 physical screen pixels for the same monitor.
pub fn logical_rect_to_physical_screen(
    rect: LogicalScreenRect,
    monitor: MonitorWorkArea,
) -> PhysicalScreenRect {
    let scale = monitor.valid_scale();
    let logical_origin_x = f64::from(monitor.monitor_px.left) / scale;
    let logical_origin_y = f64::from(monitor.monitor_px.top) / scale;
    let left = round_i32(f64::from(monitor.monitor_px.left) + (rect.x - logical_origin_x) * scale);
    let top = round_i32(f64::from(monitor.monitor_px.top) + (rect.y - logical_origin_y) * scale);
    let right = round_i32(
        f64::from(monitor.monitor_px.left) + (rect.x + rect.width - logical_origin_x) * scale,
    );
    let bottom = round_i32(
        f64::from(monitor.monitor_px.top) + (rect.y + rect.height - logical_origin_y) * scale,
    );
    PhysicalScreenRect {
        left,
        top,
        right,
        bottom,
    }
}

fn round_i32(value: f64) -> i32 {
    value
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

pub fn logical_size_to_physical(width: f64, height: f64, scale_factor: f64) -> PhysicalPixelSize {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    PhysicalPixelSize {
        width: round_i32(width * scale).max(1),
        height: round_i32(height * scale).max(1),
    }
}

pub fn hud_origin_px(
    work_area: PhysicalScreenRect,
    hud_size: PhysicalPixelSize,
    scale_factor: f64,
    bottom_gap_dip: f64,
) -> PhysicalScreenPoint {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let available_width = work_area.width().max(0);
    let x = work_area.left + (available_width - hud_size.width).max(0) / 2;
    let gap = round_i32(bottom_gap_dip * scale).max(0);
    let y = (work_area.bottom - hud_size.height - gap).max(work_area.top);
    PhysicalScreenPoint { x, y }
}

pub fn assistant_origin_px(
    work_area: PhysicalScreenRect,
    selection: Option<PhysicalScreenRect>,
    window_size: PhysicalPixelSize,
    scale_factor: f64,
) -> PhysicalScreenPoint {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let margin = round_i32(12.0 * scale).max(0);
    let gap = round_i32(8.0 * scale).max(0);
    let min_x = work_area.left + margin;
    let max_x = (work_area.right - window_size.width - margin).max(min_x);
    let min_y = work_area.top + margin;
    let max_y = (work_area.bottom - window_size.height - margin).max(min_y);

    let Some(selection) = selection else {
        return PhysicalScreenPoint {
            x: (work_area.left + (work_area.width() - window_size.width) / 2).clamp(min_x, max_x),
            y: (work_area.top + (work_area.height() - window_size.height) / 3).clamp(min_y, max_y),
        };
    };

    let center_x = i64::from(selection.left) + i64::from(selection.width()) / 2;
    let x = (center_x - i64::from(window_size.width) / 2).clamp(i64::from(min_x), i64::from(max_x))
        as i32;
    let below_y = selection.bottom.saturating_add(gap);
    let above_y = selection.top.saturating_sub(window_size.height + gap);
    let y = if below_y <= max_y {
        below_y
    } else if above_y >= min_y {
        above_y
    } else {
        below_y.clamp(min_y, max_y)
    };
    PhysicalScreenPoint { x, y }
}

#[derive(Debug, Clone)]
pub struct ForegroundWindowInfo {
    pub hwnd: HWND,
    pub process_id: u32,
    pub app_name: Option<String>,
}

/// Returns foreground HWND/PID/application executable name. Window titles are never read.
pub fn foreground_window() -> Option<ForegroundWindowInfo> {
    let hwnd = foreground_window_handle()?;
    let mut process_id = 0;
    if unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) } == 0 || process_id == 0 {
        return None;
    }
    let app_name = process_image_path(process_id).and_then(|path| {
        path.file_stem()
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string_lossy().into_owned())
    });
    Some(ForegroundWindowInfo {
        hwnd,
        process_id,
        app_name,
    })
}

pub fn foreground_window_handle() -> Option<HWND> {
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid()).then_some(hwnd)
}

pub fn foreground_window_exists() -> bool {
    foreground_window_handle().is_some()
}

/// Whether the current foreground GUI thread exposes an enabled keyboard-focus window.
pub fn foreground_has_keyboard_focus() -> bool {
    foreground_keyboard_focus_window()
        .is_some_and(|focused| unsafe { IsWindowEnabled(focused) }.as_bool())
}

fn foreground_keyboard_focus_window() -> Option<HWND> {
    let foreground = foreground_window_handle()?;
    let thread_id = unsafe { GetWindowThreadProcessId(foreground, None) };
    if thread_id == 0 {
        return None;
    }
    let mut info = GUITHREADINFO {
        cbSize: size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetGUIThreadInfo(thread_id, &mut info) }.is_err() || info.hwndFocus.is_invalid() {
        return None;
    }
    Some(info.hwndFocus)
}

/// Detect the Win32 read-only style for standard Edit and RichEdit controls. Custom controls are
/// intentionally left as unknown rather than rejected.
pub fn foreground_focus_is_known_read_only() -> bool {
    let Some(focused) = foreground_keyboard_focus_window() else {
        return false;
    };
    let mut class_name = [0u16; 64];
    let length = unsafe { GetClassNameW(focused, &mut class_name) };
    if length <= 0 {
        return false;
    }
    let class_name = String::from_utf16_lossy(&class_name[..length as usize]).to_ascii_lowercase();
    let is_edit = class_name == "edit" || class_name.starts_with("richedit");
    const ES_READONLY: u32 = 0x0800;
    is_edit && (unsafe { GetWindowLongPtrW(focused, GWL_STYLE) } as u32 & ES_READONLY != 0)
}

pub fn taskbar_uses_dark_theme() -> bool {
    let mut value = 1u32;
    let mut bytes = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut bytes),
        )
    };
    status == ERROR_SUCCESS && bytes == size_of::<u32>() as u32 && value == 0
}

pub fn apps_use_dark_theme() -> bool {
    let mut value = 1u32;
    let mut bytes = size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut bytes),
        )
    };
    status == ERROR_SUCCESS && bytes == size_of::<u32>() as u32 && value == 0
}

fn process_image_path(process_id: u32) -> Option<PathBuf> {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let handle = OwnedHandle(handle);
    let mut buffer = vec![0u16; PROCESS_IMAGE_BUFFER_U16];
    let mut len = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle.0,
            Default::default(),
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()?;
    buffer.truncate(len as usize);
    Some(PathBuf::from(OsString::from_wide(&buffer)))
}

pub fn monitor_work_area_for_window(hwnd: HWND) -> Option<MonitorWorkArea> {
    if hwnd.is_invalid() {
        return None;
    }
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }

    let mut dpi_x = 0;
    let mut dpi_y = 0;
    let monitor_dpi =
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }
            .is_ok()
            .then_some(dpi_x)
            .filter(|dpi| *dpi > 0);
    let window_dpi = unsafe { GetDpiForWindow(hwnd) };
    let dpi = monitor_dpi
        .or((window_dpi > 0).then_some(window_dpi))
        .unwrap_or(96);

    Some(MonitorWorkArea {
        monitor_px: info.rcMonitor.into(),
        work_area_px: info.rcWork.into(),
        scale_factor: f64::from(dpi) / BASE_DPI,
    })
}

pub fn foreground_monitor_work_area() -> Option<MonitorWorkArea> {
    foreground_window_handle().and_then(monitor_work_area_for_window)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IntegrityLevel {
    rid: u32,
}

impl IntegrityLevel {
    pub const fn from_rid(rid: u32) -> Self {
        Self { rid }
    }

    pub const fn rid(self) -> u32 {
        self.rid
    }

    pub fn label(self) -> &'static str {
        match self.rid {
            0x0000..=0x0fff => "untrusted",
            0x1000..=0x1fff => "low",
            0x2000..=0x20ff => "medium",
            0x2100..=0x2fff => "medium_plus",
            0x3000..=0x3fff => "high",
            0x4000..=0x4fff => "system",
            _ => "protected_or_unknown",
        }
    }
}

pub fn current_process_integrity() -> Result<IntegrityLevel, String> {
    integrity_for_process(unsafe { GetCurrentProcess() })
}

pub fn process_integrity(process_id: u32) -> Result<IntegrityLevel, String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
        .map_err(|_| "target_process_unavailable".to_string())?;
    let process = OwnedHandle(process);
    integrity_for_process(process.0)
}

fn integrity_for_process(process: HANDLE) -> Result<IntegrityLevel, String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }
        .map_err(|_| "process_token_unavailable".to_string())?;
    let token = OwnedHandle(token);

    let mut required = 0;
    let _ = unsafe { GetTokenInformation(token.0, TokenIntegrityLevel, None, 0, &mut required) };
    if required < size_of::<TOKEN_MANDATORY_LABEL>() as u32 {
        return Err("integrity_information_unavailable".into());
    }

    // `usize` storage gives the variable-sized token result pointer alignment suitable for the
    // TOKEN_MANDATORY_LABEL header followed by its SID.
    let words = (required as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0usize; words];
    unsafe {
        GetTokenInformation(
            token.0,
            TokenIntegrityLevel,
            Some(buffer.as_mut_ptr().cast()),
            required,
            &mut required,
        )
    }
    .map_err(|_| "integrity_information_unavailable".to_string())?;

    let label = unsafe { &*buffer.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    let sid = label.Label.Sid;
    if sid.is_invalid() {
        return Err("integrity_sid_invalid".into());
    }
    let count_ptr = unsafe { GetSidSubAuthorityCount(sid) };
    if count_ptr.is_null() {
        return Err("integrity_sid_invalid".into());
    }
    let count = unsafe { *count_ptr };
    if count == 0 {
        return Err("integrity_sid_invalid".into());
    }
    let rid_ptr = unsafe { GetSidSubAuthority(sid, u32::from(count - 1)) };
    if rid_ptr.is_null() {
        return Err("integrity_sid_invalid".into());
    }
    Ok(IntegrityLevel::from_rid(unsafe { *rid_ptr }))
}

pub fn uipi_blocks_injection(source: IntegrityLevel, target: IntegrityLevel) -> bool {
    target > source
}

/// Returns whether SendInput may target the current foreground process under UIPI.
///
/// Error strings are stable non-sensitive classifications for diagnostics/error mapping.
pub fn can_inject_foreground() -> Result<bool, String> {
    let target = foreground_window().ok_or_else(|| "no_foreground_window".to_string())?;
    if target.process_id == std::process::id() {
        return Ok(true);
    }
    let source_integrity = current_process_integrity()?;
    let target_integrity = process_integrity(target.process_id)?;
    Ok(!uipi_blocks_injection(source_integrity, target_integrity))
}

/// Load both native icon sizes from the executable's multi-size ICO resource for this window's
/// current DPI. Tauri 2.11 decodes only the first ICO frame (16px) for its default window icon,
/// which Windows otherwise scales up for both the title bar and taskbar.
pub fn configure_app_window_icons(hwnd: HWND, use_ink: bool) -> Result<(), String> {
    if hwnd.is_invalid() {
        return Err("invalid_app_window".into());
    }

    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(BASE_DPI as u32);
    let small_width = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) };
    let small_height = unsafe { GetSystemMetricsForDpi(SM_CYSMICON, dpi) };
    let big_width = unsafe { GetSystemMetricsForDpi(SM_CXICON, dpi) };
    let big_height = unsafe { GetSystemMetricsForDpi(SM_CYICON, dpi) };
    if [small_width, small_height, big_width, big_height]
        .into_iter()
        .any(|dimension| dimension <= 0)
    {
        return Err("app_icon_metrics_unavailable".into());
    }

    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|_| "app_icon_module_unavailable".to_string())?;
    let instance = HINSTANCE(module.0);
    let small = if use_ink {
        create_icon_from_ico(INK_ICON_ICO, small_width, small_height)
            .map_err(|_| "app_icon_small_load_failed".to_string())?
    } else {
        HICON(
            unsafe {
                LoadImageW(
                    Some(instance),
                    IDI_APPLICATION,
                    IMAGE_ICON,
                    small_width,
                    small_height,
                    LR_SHARED,
                )
            }
            .map_err(|_| "app_icon_small_load_failed".to_string())?
            .0,
        )
    };
    let big = match unsafe {
        LoadImageW(
            Some(instance),
            IDI_APPLICATION,
            IMAGE_ICON,
            big_width,
            big_height,
            LR_SHARED,
        )
    } {
        Ok(icon) => HICON(icon.0),
        Err(_) => {
            if use_ink {
                let _ = unsafe { DestroyIcon(small) };
            }
            return Err("app_icon_big_load_failed".into());
        }
    };

    unsafe {
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(LPARAM(small.0 as isize)),
        );
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(LPARAM(big.0 as isize)),
        );
    }

    let icons = CUSTOM_APP_ICONS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut icons) = icons.lock() {
        if let Some(old_small) = icons.remove(&(hwnd.0 as isize)) {
            let _ = unsafe { DestroyIcon(HICON(old_small as *mut _)) };
        }
        if use_ink {
            icons.insert(hwnd.0 as isize, small.0 as isize);
        }
    }
    Ok(())
}

fn create_icon_from_ico(
    ico: &[u8],
    desired_width: i32,
    desired_height: i32,
) -> windows::core::Result<HICON> {
    let image = ico_image_for_size(ico, desired_width as u16, desired_height as u16)
        .ok_or_else(windows::core::Error::from_win32)?;
    unsafe {
        CreateIconFromResourceEx(
            image,
            true,
            0x0003_0000,
            desired_width,
            desired_height,
            LR_DEFAULTCOLOR,
        )
    }
}

fn ico_image_for_size(ico: &[u8], width: u16, height: u16) -> Option<&[u8]> {
    if ico.get(0..4)? != [0, 0, 1, 0] {
        return None;
    }
    let count = u16::from_le_bytes(ico.get(4..6)?.try_into().ok()?) as usize;
    let (_, offset, length) = (0..count)
        .filter_map(|index| {
            let entry = ico.get(6 + index * 16..6 + (index + 1) * 16)?;
            let entry_width = if entry[0] == 0 {
                256
            } else {
                u16::from(entry[0])
            };
            let entry_height = if entry[1] == 0 {
                256
            } else {
                u16::from(entry[1])
            };
            let length = u32::from_le_bytes(entry.get(8..12)?.try_into().ok()?) as usize;
            let offset = u32::from_le_bytes(entry.get(12..16)?.try_into().ok()?) as usize;
            let score = entry_width.abs_diff(width) + entry_height.abs_diff(height);
            Some((score, offset, length))
        })
        .min_by_key(|(score, _, _)| *score)?;
    ico.get(offset..offset.checked_add(length)?)
}

/// Repaint the non-client frame after changing a Tauri window theme. DWM can apply the new
/// immersive-dark-mode attribute without repainting the active title bar until activation changes.
pub fn redraw_window_frame(hwnd: HWND) -> Result<(), String> {
    if hwnd.is_invalid() {
        return Err("invalid_app_window".into());
    }

    // Re-run non-client activation painting without changing the final active state. A plain
    // RDW_FRAME does not reliably make an active Windows 11 title bar consume the new DWM theme.
    let is_foreground = unsafe { GetForegroundWindow() } == hwnd;
    let activation_states = if is_foreground {
        [false, true]
    } else {
        [true, false]
    };
    for active in activation_states {
        unsafe {
            SendMessageW(
                hwnd,
                WM_NCACTIVATE,
                Some(WPARAM(active as usize)),
                Some(LPARAM(0)),
            );
        }
    }

    let redrawn = unsafe {
        RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_FRAME | RDW_INVALIDATE | RDW_UPDATENOW,
        )
    };
    if redrawn.as_bool() {
        Ok(())
    } else {
        Err("app_window_frame_redraw_failed".into())
    }
}

pub fn configure_hud_window(hwnd: HWND) -> Result<(), String> {
    if hwnd.is_invalid() {
        return Err("invalid_hud_window".into());
    }
    let current = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    let required = WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
    if current & required != required {
        unsafe { SetLastError(ERROR_SUCCESS) };
        let previous =
            unsafe { SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (current | required) as isize) };
        if previous == 0 && unsafe { GetLastError() } != ERROR_SUCCESS {
            return Err("set_hud_style_failed".into());
        }
    }
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
        )
    }
    .map_err(|_| "set_hud_topmost_failed".to_string())
}

pub fn place_window_px(
    hwnd: HWND,
    origin: PhysicalScreenPoint,
    size: PhysicalPixelSize,
    topmost: bool,
    no_activate: bool,
) -> Result<(), String> {
    if hwnd.is_invalid() {
        return Err("invalid_window".into());
    }
    let mut flags = SWP_NOOWNERZORDER;
    if no_activate {
        flags |= SWP_NOACTIVATE;
    }
    unsafe {
        SetWindowPos(
            hwnd,
            topmost.then_some(HWND_TOPMOST),
            origin.x,
            origin.y,
            size.width,
            size.height,
            flags,
        )
    }
    .map_err(|_| "set_window_position_failed".to_string())
}

pub fn shell_open(target: &OsStr) -> Result<(), String> {
    let wide = wide_null(target);
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("shell_open_failed".into())
    }
}

pub fn shell_open_path(path: &Path) -> Result<(), String> {
    shell_open(path.as_os_str())
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

pub fn microphone_access_allowed() -> bool {
    microphone_consent_value()
        .as_deref()
        .is_none_or(microphone_registry_value_allows)
}

fn microphone_consent_value() -> Option<String> {
    let mut bytes = 0;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone"
            ),
            w!("Value"),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if status != ERROR_SUCCESS || bytes < 2 {
        return None;
    }
    let mut buffer = vec![0u16; (bytes as usize).div_ceil(size_of::<u16>())];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone"
            ),
            w!("Value"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = (bytes as usize / size_of::<u16>()).min(buffer.len());
    buffer.truncate(len);
    while buffer.last() == Some(&0) {
        buffer.pop();
    }
    Some(String::from_utf16_lossy(&buffer))
}

fn microphone_registry_value_allows(value: &str) -> bool {
    !value.eq_ignore_ascii_case("deny")
}

pub fn capability_diagnostics() -> Vec<PlatformCapabilityStatus> {
    let integrity = current_process_integrity();
    vec![
        PlatformCapabilityStatus::available("keyboard_hook", "WH_KEYBOARD_LL"),
        PlatformCapabilityStatus::available("send_input", "SendInput"),
        PlatformCapabilityStatus::available("ui_automation", "UI Automation"),
        // Diagnostics are requested through an active WebView, so the runtime is demonstrably
        // available without probing user-specific installation paths.
        PlatformCapabilityStatus::available("webview2", "Evergreen runtime active"),
        match integrity {
            Ok(level) => PlatformCapabilityStatus::available("integrity", level.label()),
            Err(classification) => {
                PlatformCapabilityStatus::unavailable("integrity", classification)
            }
        },
    ]
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secondary_monitor() -> MonitorWorkArea {
        MonitorWorkArea {
            monitor_px: PhysicalScreenRect {
                left: -2560,
                top: -300,
                right: 0,
                bottom: 1140,
            },
            work_area_px: PhysicalScreenRect {
                left: -2560,
                top: -300,
                right: 0,
                bottom: 1100,
            },
            scale_factor: 1.5,
        }
    }

    #[test]
    fn physical_logical_round_trip_preserves_negative_mixed_dpi_rect() {
        let monitor = secondary_monitor();
        let physical = PhysicalScreenRect {
            left: -2400,
            top: -150,
            right: -1800,
            bottom: 150,
        };
        let logical = physical_rect_to_logical_screen(physical, monitor);
        assert_eq!(logical.x, -1600.0);
        assert_eq!(logical.y, -100.0);
        assert_eq!(logical.width, 400.0);
        assert_eq!(logical.height, 200.0);
        assert_eq!(logical_rect_to_physical_screen(logical, monitor), physical);
    }

    #[test]
    fn hud_uses_work_area_scale_and_negative_origin() {
        let monitor = secondary_monitor();
        let size = logical_size_to_physical(352.0, 76.0, monitor.scale_factor);
        let origin = hud_origin_px(monitor.work_area_px, size, monitor.scale_factor, 32.0);
        assert_eq!(
            size,
            PhysicalPixelSize {
                width: 528,
                height: 114
            }
        );
        assert_eq!(origin, PhysicalScreenPoint { x: -1544, y: 938 });
    }

    #[test]
    fn assistant_moves_above_selection_when_below_would_overflow() {
        let monitor = secondary_monitor();
        let size = logical_size_to_physical(560.0, 136.0, monitor.scale_factor);
        let selection = PhysicalScreenRect {
            left: -1200,
            top: 1020,
            right: -900,
            bottom: 1080,
        };
        let origin = assistant_origin_px(
            monitor.work_area_px,
            Some(selection),
            size,
            monitor.scale_factor,
        );
        assert_eq!(origin.x, -1470);
        assert_eq!(origin.y, 804);
    }

    #[test]
    fn integrity_comparison_only_blocks_write_up() {
        let medium = IntegrityLevel::from_rid(0x2000);
        let high = IntegrityLevel::from_rid(0x3000);
        assert!(uipi_blocks_injection(medium, high));
        assert!(!uipi_blocks_injection(high, medium));
        assert!(!uipi_blocks_injection(medium, medium));
    }

    #[test]
    fn microphone_registry_only_blocks_explicit_deny() {
        assert!(microphone_registry_value_allows("Allow"));
        assert!(microphone_registry_value_allows("Prompt"));
        assert!(!microphone_registry_value_allows("Deny"));
    }

    #[test]
    fn ink_icon_contains_every_windows_dpi_frame() {
        for size in [16, 24, 32, 48, 64, 256] {
            let image = ico_image_for_size(INK_ICON_ICO, size, size).unwrap();
            assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        }
        assert_eq!(
            &ico_image_for_size(INK_ICON_ICO, 20, 20).unwrap()[..8],
            b"\x89PNG\r\n\x1a\n"
        );
        assert!(ico_image_for_size(b"not an ico", 24, 24).is_none());
    }

    #[test]
    fn windows_can_decode_ink_icon_resource() {
        let icon = create_icon_from_ico(INK_ICON_ICO, 24, 24).unwrap();
        unsafe { DestroyIcon(icon) }.unwrap();
    }
}
