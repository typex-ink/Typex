#![cfg(target_os = "windows")]

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use typex_lib::error::ErrorCode;
use typex_lib::platform::focus::FocusTarget;
use windows::Win32::Foundation::{FreeLibrary, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::LoadLibraryW;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_C, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CW_USEDEFAULT, CreateWindowExW, DestroyWindow, DispatchMessageW, GWL_EXSTYLE,
    GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SW_SHOW,
    SW_SHOWNOACTIVATE, SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_KEYDOWN, WM_QUIT, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, w};

const EM_SETSEL: u32 = 0x00b1;
const FALLBACK_SELECTION: &str = "fallback selection";

#[derive(Clone, Copy)]
enum HarnessKind {
    Edit { read_only: bool, rich_edit: bool },
    NoTextPattern,
    Hud,
}

struct EditHarness {
    hwnd: HWND,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

struct ThreadInputAttachment {
    source: u32,
    target: u32,
}

impl ThreadInputAttachment {
    fn attach(source: u32, target: u32) -> Option<Self> {
        if source == 0 || target == 0 || source == target {
            return None;
        }
        unsafe { AttachThreadInput(source, target, true) }
            .as_bool()
            .then_some(Self { source, target })
    }
}

impl Drop for ThreadInputAttachment {
    fn drop(&mut self) {
        let _ = unsafe { AttachThreadInput(self.source, self.target, false) };
    }
}

impl EditHarness {
    fn spawn(title: &'static str, read_only: bool, rich_edit: bool) -> Self {
        Self::spawn_kind(
            title,
            HarnessKind::Edit {
                read_only,
                rich_edit,
            },
        )
    }

    fn spawn_no_text_pattern(title: &'static str) -> Self {
        Self::spawn_kind(title, HarnessKind::NoTextPattern)
    }

    fn spawn_hud(title: &'static str) -> Self {
        Self::spawn_kind(title, HarnessKind::Hud)
    }

    fn spawn_kind(title: &'static str, kind: HarnessKind) -> Self {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name(format!("typex-win32-harness-{title}"))
            .spawn(move || {
                let rich_edit = matches!(
                    kind,
                    HarnessKind::Edit {
                        rich_edit: true,
                        ..
                    }
                );
                let rich_edit_module = rich_edit
                    .then(|| unsafe { LoadLibraryW(w!("Msftedit.dll")) })
                    .transpose()
                    .expect("load system RichEdit provider");
                let class_name = match kind {
                    HarnessKind::Edit {
                        rich_edit: true, ..
                    } => w!("RICHEDIT50W"),
                    HarnessKind::Edit { .. } => w!("EDIT"),
                    HarnessKind::NoTextPattern => w!("BUTTON"),
                    HarnessKind::Hud => w!("STATIC"),
                };
                let read_only = matches!(
                    kind,
                    HarnessKind::Edit {
                        read_only: true,
                        ..
                    }
                );
                let hwnd = unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        class_name,
                        w!(""),
                        WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | if read_only { 0x0800 } else { 0 }),
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        640,
                        240,
                        None,
                        None,
                        None,
                        None,
                    )
                }
                .expect("create Win32 harness window");
                ready_tx
                    .send((unsafe { GetCurrentThreadId() }, hwnd.0 as isize))
                    .expect("publish harness HWND");

                let mut message = MSG::default();
                loop {
                    let status = unsafe { GetMessageW(&mut message, None, 0, 0) };
                    if status.0 <= 0 {
                        break;
                    }
                    if matches!(kind, HarnessKind::NoTextPattern)
                        && message.message == WM_KEYDOWN
                        && message.wParam.0 == usize::from(VK_C.0)
                        && unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0
                    {
                        let mut clipboard = arboard::Clipboard::new()
                            .expect("open clipboard from no-TextPattern fixture");
                        clipboard
                            .set_text(FALLBACK_SELECTION)
                            .expect("publish fixture selection to clipboard");
                    }
                    unsafe {
                        let _ = TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                let _ = unsafe { DestroyWindow(hwnd) };
                if let Some(module) = rich_edit_module {
                    let _ = unsafe { FreeLibrary(module) };
                }
            })
            .expect("spawn Win32 harness thread");
        let (thread_id, raw_hwnd) = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Win32 harness startup");
        let harness = Self {
            hwnd: HWND(raw_hwnd as *mut _),
            thread_id,
            thread: Some(thread),
        };
        if !matches!(kind, HarnessKind::Hud) {
            harness.activate();
        }
        harness
    }

    fn activate(&self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let current_thread = unsafe { GetCurrentThreadId() };
            let mut message = MSG::default();
            unsafe {
                let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            }
            let foreground = unsafe { GetForegroundWindow() };
            let foreground_thread = if foreground.is_invalid() {
                0
            } else {
                unsafe { GetWindowThreadProcessId(foreground, None) }
            };
            let _foreground_attachment =
                ThreadInputAttachment::attach(current_thread, foreground_thread);
            let _owner_attachment = ThreadInputAttachment::attach(current_thread, self.thread_id);
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOW);
                let _ = BringWindowToTop(self.hwnd);
                let _ = SetForegroundWindow(self.hwnd);
                let _ = SetFocus(Some(self.hwnd));
            }
            if unsafe { GetForegroundWindow() } == self.hwnd {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("Win32 harness did not become foreground");
    }

    fn set_text(&self, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { SetWindowTextW(self.hwnd, PCWSTR(wide.as_ptr())) }.expect("set harness text");
    }

    fn select(&self, start: usize, end: usize) {
        unsafe {
            SendMessageW(
                self.hwnd,
                EM_SETSEL,
                Some(WPARAM(start)),
                Some(LPARAM(end as isize)),
            );
        }
    }

    fn text(&self) -> String {
        let length = unsafe { GetWindowTextLengthW(self.hwnd) }.max(0) as usize;
        let mut buffer = vec![0u16; length + 1];
        let copied = unsafe { GetWindowTextW(self.hwnd, &mut buffer) }.max(0) as usize;
        String::from_utf16_lossy(&buffer[..copied])
    }

    fn wait_for_text(&self, expected: &str) {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let actual = self.text();
            if actual == expected {
                return;
            }
            if Instant::now() >= deadline {
                assert_eq!(actual, expected, "Win32 control text did not settle");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for EditHarness {
    fn drop(&mut self) {
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
#[ignore = "uses the interactive Windows desktop and real clipboard"]
fn win32_uia_sendinput_and_focus_contract() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();
    let primary = EditHarness::spawn("primary", false, true);
    primary.set_text("alpha beta");
    primary.select(0, 5);
    let target = FocusTarget::capture().expect("capture primary harness target");

    let selection = typex_lib::selection::platform_default();
    assert_eq!(
        selection.read_targeted(Some(&target)).unwrap().as_deref(),
        Some("alpha")
    );
    assert!(selection.read_bounds().unwrap().is_some());

    let hud = EditHarness::spawn_hud("hud");
    primary.activate();
    let foreground_before_hud = unsafe { GetForegroundWindow() };
    typex_lib::platform::windows::configure_hud_window(hud.hwnd)
        .expect("configure ordinary Win32 window as HUD");
    unsafe {
        let _ = ShowWindow(hud.hwnd, SW_SHOWNOACTIVATE);
    }
    std::thread::sleep(Duration::from_millis(50));
    let hud_style = unsafe { GetWindowLongPtrW(hud.hwnd, GWL_EXSTYLE) } as u32;
    let required_hud_style = WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TOPMOST.0;
    assert_eq!(hud_style & required_hud_style, required_hud_style);
    assert_eq!(unsafe { GetForegroundWindow() }, foreground_before_hud);

    primary.set_text("");
    typex_lib::inject::windows::send_unicode_to("A中😀", Some(&target)).unwrap();
    primary.wait_for_text("A中😀");

    primary.set_text("");
    typex_lib::inject::windows::paste_text_to("paste 文本", 10, Some(&target)).unwrap();
    primary.wait_for_text("paste 文本");

    let clipboard_text_before_fallback = {
        let mut clipboard = arboard::Clipboard::new().expect("open clipboard before fallback read");
        clipboard.get_text().ok()
    };
    let _fallback = EditHarness::spawn_no_text_pattern("no-text-pattern");
    let fallback_target = FocusTarget::capture().expect("capture fallback harness target");
    let fallback_selection = typex_lib::selection::platform_default();
    assert_eq!(
        fallback_selection
            .read_targeted(Some(&fallback_target))
            .unwrap()
            .as_deref(),
        Some(FALLBACK_SELECTION)
    );
    assert_eq!(fallback_selection.read_bounds().unwrap(), None);
    let clipboard_text_after_fallback = {
        let mut clipboard = arboard::Clipboard::new().expect("open clipboard after fallback read");
        clipboard.get_text().ok()
    };
    assert_eq!(
        clipboard_text_after_fallback, clipboard_text_before_fallback,
        "Ctrl+C fallback must restore the clipboard text it replaced"
    );

    let secondary = EditHarness::spawn("secondary", false, false);
    let error = typex_lib::inject::windows::send_unicode_to("wrong", Some(&target)).unwrap_err();
    assert_eq!(error.code, ErrorCode::NoFocus);
    assert_eq!(primary.text(), "paste 文本");
    assert_eq!(secondary.text(), "");

    let read_only = EditHarness::spawn("read-only", true, false);
    read_only.set_text("read-only selection");
    read_only.select(0, 9);
    let read_only_target = FocusTarget::capture().expect("capture read-only harness target");
    let read_only_selection = typex_lib::selection::platform_default();
    assert_eq!(
        read_only_selection
            .read_targeted(Some(&read_only_target))
            .unwrap()
            .as_deref(),
        Some("read-only")
    );
    let error =
        typex_lib::inject::windows::send_unicode_to("must not appear", Some(&read_only_target))
            .unwrap_err();
    assert_eq!(error.code, ErrorCode::NoFocus);
    assert_eq!(read_only.text(), "read-only selection");
}
